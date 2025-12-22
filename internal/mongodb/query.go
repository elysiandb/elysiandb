package mongodb

import (
	"context"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
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
	ctx := context.Background()

	includeAll, paths := ParseIncludes(includesParam)
	rootSpecs := BuildSpecsFromSample(entity, includeAll, paths)

	leafPaths := make([][]string, 0)
	for _, p := range paths {
		if len(p) > 1 {
			leafPaths = append(leafPaths, p)
		}
	}

	if len(rootSpecs) == 0 {
		opts := FindOptions(limit, offset, sortField, sortAscending)

		cur, err := globals.MongoDB.Collection(entity).Find(ctx, expr, opts)
		if err != nil || cur == nil {
			return []map[string]any{}
		}
		defer cur.Close(ctx)

		out := make([]map[string]any, 0)
		for cur.Next(ctx) {
			var raw map[string]any
			if cur.Decode(&raw) == nil {
				out = append(out, NormalizeMongoDocument(raw))
			}
		}

		if includeAll {
			ResolveIncludesAllRecursive(out, 8)
		} else if len(leafPaths) > 0 {
			ResolveIncludesPaths(out, leafPaths)
		}

		return out
	}

	pipeline := make([]bson.D, 0, 10+len(rootSpecs)*3)
	pipeline = append(pipeline, bson.D{{Key: "$match", Value: expr}})

	if sortField != "" {
		dir := 1
		if !sortAscending {
			dir = -1
		}
		pipeline = append(pipeline, bson.D{{Key: "$sort", Value: bson.D{{Key: sortField, Value: dir}}}})
	}
	if offset > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$skip", Value: int64(offset)}})
	}
	if limit > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$limit", Value: int64(limit)}})
	}

	addFields := bson.D{}
	for _, sp := range rootSpecs {
		field := "$" + sp.Path[0]
		addFields = append(addFields, bson.E{
			Key: sp.Tmp,
			Value: bson.D{
				{Key: "$cond", Value: bson.D{
					{Key: "if", Value: bson.D{{Key: "$isArray", Value: field}}},
					{Key: "then", Value: bson.D{{Key: "$map", Value: bson.D{
						{Key: "input", Value: field},
						{Key: "as", Value: "r"},
						{Key: "in", Value: bson.D{{Key: "$ifNull", Value: bson.A{
							"$$r.id",
							"$$r._id",
						}}}},
					}}}},
					{Key: "else", Value: bson.D{{Key: "$ifNull", Value: bson.A{
						field + ".id",
						"$" + sp.Path[0] + "Id",
					}}}},
				}},
			},
		})
	}
	if len(addFields) > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$addFields", Value: addFields}})
	}

	project := bson.D{}
	for _, sp := range rootSpecs {
		project = append(project, bson.E{Key: sp.Tmp, Value: 0})
	}

	for _, sp := range rootSpecs {
		pipeline = append(pipeline,
			bson.D{{Key: "$lookup", Value: bson.M{
				"from":         sp.From,
				"localField":   sp.Tmp,
				"foreignField": "_id",
				"as":           sp.As,
			}}},
		)
	}

	if len(project) > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$project", Value: project}})
	}

	cur, err := globals.MongoDB.Collection(entity).Aggregate(ctx, pipeline)
	if err != nil || cur == nil {
		return []map[string]any{}
	}
	defer cur.Close(ctx)

	out := make([]map[string]any, 0)
	for cur.Next(ctx) {
		var raw map[string]any
		if cur.Decode(&raw) == nil {
			out = append(out, NormalizeMongoDocument(raw))
		}
	}

	out = AddIncludeEntityTags(out, rootSpecs)

	if includeAll {
		ResolveIncludesAllRecursive(out, 8)
	} else if len(leafPaths) > 0 {
		ResolveIncludesPaths(out, leafPaths)
	}

	return out
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
	out := bson.M{}

	for field, ops := range leaf {
		for op, val := range ops {
			switch op {
			case "eq":
				out[field] = BuildMongoEq(val)
			}
		}
	}

	return out
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
