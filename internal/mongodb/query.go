package mongodb

import (
	"strings"

	"github.com/taymour/elysiandb/internal/query"
	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

func ExecuteQuery(q query.Query) ([]map[string]any, error) {
	sortField := ""
	sortAsc := true

	for f, dir := range q.Sorts {
		sortField = f
		sortAsc = strings.ToLower(dir) != "desc"
		break
	}

	mongoExpr := BuildMongoExpr(q.Filter)

	return ListEntitiesWithExpr(
		q.Entity,
		q.Limit,
		q.Offset,
		sortField,
		sortAsc,
		mongoExpr,
		"",
		"",
	), nil
}

func ListEntitiesWithExpr(
	entity string,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	expr bson.M,
	search string,
	includesParam string,
) []map[string]any {
	return listEntitiesCore(entity, limit, offset, sortField, sortAscending, expr, search, includesParam)
}

func BuildMongoFilterNode(node query.FilterNode) bson.M {
	if node.Leaf != nil {
		return BuildMongoLeaf(node.Leaf)
	}

	if len(node.And) > 0 {
		arr := make([]bson.M, 0, len(node.And))
		for _, n := range node.And {
			f := BuildMongoFilterNode(n)
			if len(f) > 0 {
				arr = append(arr, f)
			}
		}

		if len(arr) == 1 {
			return arr[0]
		}

		return bson.M{"$and": arr}
	}

	if len(node.Or) > 0 {
		arr := make([]bson.M, 0, len(node.Or))
		for _, n := range node.Or {
			f := BuildMongoFilterNode(n)
			if len(f) > 0 {
				arr = append(arr, f)
			}
		}

		if len(arr) == 1 {
			return arr[0]
		}

		return bson.M{"$or": arr}
	}

	return bson.M{}
}

func BuildMongoLeaf(leaf map[string]map[string]string) bson.M {
	return BuildMongoFilters(leaf)
}

func BuildMongoEq(val string) bson.M {
	if strings.Contains(val, "*") {
		pattern := "^" + RegexpEscape(val) + "$"
		pattern = strings.ReplaceAll(pattern, "\\*", ".*")

		return bson.M{
			"$regex":   pattern,
			"$options": "i",
		}
	}

	return bson.M{"$eq": val}
}

func RegexpEscape(s string) string {
	var b strings.Builder
	for _, r := range s {
		switch r {
		case '.', '+', '?', '(', ')', '[', ']', '{', '}', '^', '$', '|':
			b.WriteRune('\\')
		}

		b.WriteRune(r)
	}

	return b.String()
}

func BsonFindOptions(opts bson.D) options.Lister[options.FindOptions] {
	o := options.Find()
	for _, e := range opts {
		switch e.Key {
		case "sort":
			o = o.SetSort(e.Value)
		case "skip":
			o = o.SetSkip(int64(e.Value.(int)))
		case "limit":
			o = o.SetLimit(int64(e.Value.(int)))
		}
	}

	return o
}

func BuildMongoExpr(node query.FilterNode) bson.M {
	if node.Leaf != nil {
		return BuildMongoFilters(node.Leaf)
	}

	if len(node.And) > 0 {
		clauses := make([]bson.M, 0, len(node.And))
		for _, n := range node.And {
			clauses = append(clauses, BuildMongoExpr(n))
		}

		return bson.M{"$and": clauses}
	}

	if len(node.Or) > 0 {
		clauses := make([]bson.M, 0, len(node.Or))
		for _, n := range node.Or {
			clauses = append(clauses, BuildMongoExpr(n))
		}

		return bson.M{"$or": clauses}
	}

	return bson.M{}
}
