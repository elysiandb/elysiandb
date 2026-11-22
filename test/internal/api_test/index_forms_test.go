package api_test

import (
	"fmt"
	"math/rand"
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/storage"
)

func randomDiffDoc(r *rand.Rand, id string) map[string]any {
	doc := map[string]any{"id": id}
	for _, f := range []string{"a", "b", "m", "list"} {
		if r.Intn(6) == 0 {
			continue
		}

		switch f {
		case "m":
			doc[f] = map[string]any{"x": randomValue(r, 1), "y": randomValue(r, 1)}
		case "list":
			doc[f] = []any{map[string]any{"x": randomValue(r, 1)}, map[string]any{"x": randomValue(r, 1)}}
		default:
			doc[f] = randomValue(r, 0)
		}
	}

	return doc
}

func checkPlannerAgainstScan(t *testing.T, r *rand.Rand, entity string, rounds int) {
	t.Helper()

	reference := api_storage.ListEntities(entity, 0, 0, "", true, nil, "", "")
	scan := func(match func(map[string]any) bool) []string {
		out := []string{}
		for _, doc := range reference {
			if match(doc) {
				out = append(out, doc["id"].(string))
			}
		}
		return out
	}

	for i := 0; i < rounds; i++ {
		filters := randomLeaf(r)
		want := scan(func(doc map[string]any) bool { return api_storage.FiltersMatchEntity(doc, filters) })
		got := ids(api_storage.ListEntities(entity, 0, 0, "", true, filters, "", ""))
		if !reflect.DeepEqual(got, want) && !(len(got) == 0 && len(want) == 0) {
			t.Fatalf("filters %v: planner %v, scan %v", filters, got, want)
		}

		res := api_storage.Plan(api_storage.ListQuery{Entity: entity, Filters: filters, CountOnly: true})
		if res.Total != len(want) {
			t.Fatalf("filters %v: count %d, scan %d", filters, res.Total, len(want))
		}
	}

	for i := 0; i < rounds/5; i++ {
		node := query.FilterNode{Or: []query.FilterNode{{Leaf: randomLeaf(r)}, {And: []query.FilterNode{{Leaf: randomLeaf(r)}, {Leaf: randomLeaf(r)}}}}}
		want := scan(func(doc map[string]any) bool { return api_storage.QueryFilterMatchesEntity(node, doc) })
		got := ids(api_storage.PlanQuery(query.Query{Entity: entity, Filter: node}, false).Decode())
		if !reflect.DeepEqual(got, want) && !(len(got) == 0 && len(want) == 0) {
			t.Fatalf("node %+v: planner %v, scan %v", node, got, want)
		}
	}
}

func TestBitmapIndexesAnswerLikeOrderedIndexes(t *testing.T) {
	initIdxTestStore(t)

	r := rand.New(rand.NewSource(7))
	docs := make([]map[string]any, 0, 9000)
	for i := 0; i < 9000; i++ {
		docs = append(docs, randomDiffDoc(r, fmt.Sprintf("q%05d", i)))
	}
	api_storage.WriteListOfEntities("forms", docs)

	e := storage.Engine()
	for _, f := range []string{"a", "b", "m.x", "m.y", "list.x"} {
		if !e.EnsureIndex("forms", datastore.IndexSpec{Kind: datastore.IndexBitmap, Field: f}) {
			t.Fatalf("bitmap index on %s not built", f)
		}
	}

	checkPlannerAgainstScan(t, r, "forms", 150)

	for i := 0; i < 3000; i++ {
		id := fmt.Sprintf("q%05d", r.Intn(9000))
		switch r.Intn(3) {
		case 0:
			api_storage.DeleteEntityById("forms", id)
		default:
			api_storage.WriteEntity("forms", randomDiffDoc(r, id))
		}
	}

	checkPlannerAgainstScan(t, r, "forms", 150)

	for _, name := range e.IndexNames("forms") {
		if name[0] == 'f' {
			t.Fatalf("the planner should read the bitmap indexes, yet built %s", name)
		}
	}
}
