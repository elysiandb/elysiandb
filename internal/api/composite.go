package api_storage

import (
	"strings"
	"sync"
	"sync/atomic"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/query"
)

type eqSortShape struct {
	eqField   string
	eqValue   string
	sortField string
	ascending bool
}

func shapeOf(q ListQuery) (eqSortShape, bool) {
	sorts := q.Sorts
	if len(sorts) == 0 && q.SortField != "" {
		sorts = []query.Sort{{Field: q.SortField, Ascending: q.SortAsc}}
	}

	if len(sorts) != 1 || q.Search != "" || q.Offset < 0 {
		return eqSortShape{}, false
	}

	filters := q.Filters
	if q.Node != nil && !nodeIsEverything(*q.Node) {
		if len(filters) > 0 || q.Node.Leaf == nil || len(q.Node.And) > 0 || len(q.Node.Or) > 0 {
			return eqSortShape{}, false
		}
		filters = q.Node.Leaf
	}

	if len(filters) != 1 {
		return eqSortShape{}, false
	}

	for field, ops := range filters {
		cmp, ok := ops["eq"]
		if !ok || len(ops) != 1 || field == sorts[0].Field {
			return eqSortShape{}, false
		}

		if strings.ContainsAny(cmp, "*\\") {
			return eqSortShape{}, false
		}

		if _, isDate, _ := docpath.ParseDate(cmp); isDate {
			return eqSortShape{}, false
		}

		return eqSortShape{eqField: field, eqValue: cmp, sortField: sorts[0].Field, ascending: sorts[0].Ascending}, true
	}

	return eqSortShape{}, false
}

func (p *planner) compositePage(q ListQuery) ([]datastore.Doc, int, bool) {
	shape, ok := shapeOf(q)
	if !ok {
		return nil, 0, false
	}

	p.observeShape(shape)

	if p.affectedByIncludes(shape.eqField) || (len(p.refs) > 0 && p.sortPathCrossesReference(shape.sortField)) {
		return nil, 0, false
	}

	view, ok := p.snap.Index(datastore.CompositeSpec(shape.eqField, shape.sortField))
	if !ok || view.Irregular() != 0 {
		return nil, 0, false
	}

	sub := view.Sub(datastore.CompositePrefix(shape.eqValue))
	snap := p.snap
	docs := []datastore.Doc{}
	if !q.CountOnly || q.Limit > 0 || q.Offset > 0 {
		walkSortIndexFrom(sub, shape.ascending, q.Offset, func(seq uint32) bool {
			docs = append(docs, snap.Indexed(seq))
			return q.Limit <= 0 || len(docs) < q.Limit
		})
	}

	return docs, sub.Len(), true
}

const (
	compositeMinDocs  = 4096
	compositeAfterUse = 8
)

type shapeKey struct {
	entity, eqField, sortField string
}

var shapeUses sync.Map

func (p *planner) observeShape(shape eqSortShape) {
	if p.snap.Len() < compositeMinDocs {
		return
	}

	spec := datastore.CompositeSpec(shape.eqField, shape.sortField)
	if p.snap.HasIndex(spec) {
		return
	}

	v, _ := shapeUses.LoadOrStore(shapeKey{p.snap.Name(), shape.eqField, shape.sortField}, new(atomic.Int64))
	if v.(*atomic.Int64).Add(1) != compositeAfterUse {
		return
	}

	if len(p.e.IndexNames(p.snap.Name())) < maxIndexesPerEntity {
		p.e.EnsureIndexWith(p.snap.Name(), spec, datastore.OriginAdvisor)
	}
}
