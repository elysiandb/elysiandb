package mongodb

import (
	"context"

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
	ctx := context.Background()
	q := BuildMongoFilters(filters)

	includeAll, paths := ParseIncludes(includesParam)

	rootSpecs := BuildSpecsFromSample(entity, includeAll, paths)

	leafPaths := ExtractLeafIncludePaths(paths)

	if len(rootSpecs) == 0 {
		out := FindEntitiesSimple(ctx, entity, q, limit, offset, sortField, sortAscending)

		ResolveLeafIncludes(out, includeAll, leafPaths)

		return out
	}

	pipeline := BuildAggregationPipeline(
		q,
		limit,
		offset,
		sortField,
		sortAscending,
		rootSpecs,
	)

	out := ExecuteAggregation(ctx, entity, pipeline)

	out = AddIncludeEntityTags(out, rootSpecs)

	ResolveLeafIncludes(out, includeAll, leafPaths)

	return out
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
