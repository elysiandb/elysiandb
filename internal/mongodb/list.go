package mongodb

import (
	"context"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"go.mongodb.org/mongo-driver/v2/bson"
)

func ListEntities(
	entity string,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	filters map[string]map[string]string,
	search string,
	includesParam string,
) []map[string]any {
	q := BuildMongoFilters(filters)

	autoInc := api_storage.ExtractAutoIncludes(filters)
	includesParam = api_storage.MergeIncludes(includesParam, autoInc)

	return listEntitiesCore(entity, limit, offset, sortField, sortAscending, q, search, includesParam)
}

func listEntitiesCore(
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
	leafPaths := ExtractLeafIncludePaths(paths)

	needPostFilter := search != ""

	queryLimit := limit
	queryOffset := offset
	if needPostFilter {
		queryLimit = 0
		queryOffset = 0
	}

	var out []map[string]any

	if len(rootSpecs) == 0 {
		out = FindEntitiesSimple(ctx, entity, expr, queryLimit, queryOffset, sortField, sortAscending)
		ResolveLeafIncludes(out, includeAll, leafPaths)
	} else {
		pipeline := BuildAggregationPipeline(expr, queryLimit, queryOffset, sortField, sortAscending, rootSpecs)
		out = ExecuteAggregation(ctx, entity, pipeline)
		out = AddIncludeEntityTags(out, rootSpecs)
		ResolveLeafIncludes(out, includeAll, leafPaths)
	}

	if search != "" {
		filtered := make([]map[string]any, 0, len(out))
		for _, e := range out {
			if api_storage.SearchMatchesEntity(e, search) {
				filtered = append(filtered, e)
			}
		}

		out = filtered
	}

	if needPostFilter {
		out = applyOffsetLimit(out, offset, limit)
	}

	return out
}

func applyOffsetLimit(in []map[string]any, offset, limit int) []map[string]any {
	start := offset
	if start > len(in) {
		start = len(in)
	}

	end := len(in)
	if limit > 0 && start+limit < end {
		end = start + limit
	}

	return in[start:end]
}

func ExtractLeafIncludePaths(paths [][]string) [][]string {
	out := make([][]string, 0)

	for _, p := range paths {
		if len(p) > 1 {
			out = append(out, p)
		}
	}

	return out
}

func ResolveLeafIncludes(items []map[string]any, includeAll bool, leafPaths [][]string) {
	if includeAll {
		ResolveIncludesAllRecursive(items, 8)

		return
	}

	if len(leafPaths) > 0 {
		ResolveIncludesPaths(items, leafPaths)
	}
}

func FindEntitiesSimple(
	ctx context.Context,
	entity string,
	query bson.M,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
) []map[string]any {
	cur, err := globals.MongoDB.
		Collection(entity).
		Find(ctx, query, FindOptions(limit, offset, sortField, sortAscending))
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

	return out
}

func BuildAggregationPipeline(
	query bson.M,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	specs []IncludeSpec,
) []bson.D {
	pipeline := make([]bson.D, 0, 10+len(specs)*3)

	pipeline = append(pipeline, bson.D{{Key: "$match", Value: query}})

	pipeline = append(pipeline, BuildSortStage(sortField, sortAscending)...)

	pipeline = append(pipeline, BuildPagingStages(limit, offset)...)

	pipeline = append(pipeline, BuildAddFieldsStage(specs)...)

	pipeline = append(pipeline, BuildLookupStages(specs)...)

	pipeline = append(pipeline, BuildProjectStage(specs)...)

	return pipeline
}

func BuildSortStage(sortField string, sortAscending bool) []bson.D {
	if sortField == "" {
		return nil
	}

	dir := 1
	if !sortAscending {
		dir = -1
	}

	return []bson.D{
		{{Key: "$sort", Value: bson.D{{Key: sortField, Value: dir}}}},
	}
}

func BuildPagingStages(limit int, offset int) []bson.D {
	stages := make([]bson.D, 0)

	if offset > 0 {
		stages = append(stages, bson.D{{Key: "$skip", Value: int64(offset)}})
	}

	if limit > 0 {
		stages = append(stages, bson.D{{Key: "$limit", Value: int64(limit)}})
	}

	return stages
}

func BuildAddFieldsStage(specs []IncludeSpec) []bson.D {
	addFields := bson.D{}

	for _, sp := range specs {
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

	if len(addFields) == 0 {
		return nil
	}

	return []bson.D{
		{{Key: "$addFields", Value: addFields}},
	}
}

func BuildLookupStages(specs []IncludeSpec) []bson.D {
	stages := make([]bson.D, 0, len(specs))

	for _, sp := range specs {
		stages = append(stages, bson.D{
			{Key: "$lookup", Value: bson.M{
				"from":         sp.From,
				"localField":   sp.Tmp,
				"foreignField": "_id",
				"as":           sp.As,
			}},
		})
	}

	return stages
}

func BuildProjectStage(specs []IncludeSpec) []bson.D {
	project := bson.D{}

	for _, sp := range specs {
		project = append(project, bson.E{Key: sp.Tmp, Value: 0})
	}

	if len(project) == 0 {
		return nil
	}

	return []bson.D{
		{{Key: "$project", Value: project}},
	}
}

func ExecuteAggregation(
	ctx context.Context,
	entity string,
	pipeline []bson.D,
) []map[string]any {
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

	return out
}
