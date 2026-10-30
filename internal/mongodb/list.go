package mongodb

import (
	"context"
	"fmt"
	"maps"
	"strings"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

const (
	scanBatchSize    = 256
	minScanBatchSize = 16
	maxScanBatchSize = 1024
	sortKeyField     = globals.CoreFieldsPrefix + "sort_key"
	datePattern      = `^[0-9]{4}-[0-9]{2}-[0-9]{2}(T[0-9]{2}:[0-9]{2}:[0-9]{2}([.,][0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2}))?\z`
)

type scanRequest struct {
	entity        string
	filter        bson.M
	sortField     string
	sortAscending bool
	offset        int
	limit         int
	batchSize     int
}

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
	return ListEntitiesWithGuard(entity, limit, offset, sortField, sortAscending, filters, search, includesParam, nil)
}

func ListEntitiesWithGuard(
	entity string,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	filters map[string]map[string]string,
	search string,
	includesParam string,
	guard api_storage.ReadGuard,
) []map[string]any {
	includesParam = api_storage.MergeIncludes(includesParam, api_storage.ExtractAutoIncludes(filters))
	paginateFirst := len(filters) == 0

	req := scanRequest{
		entity:        entity,
		filter:        bson.M{},
		sortField:     sortField,
		sortAscending: sortAscending,
	}

	if search == "" {
		req.filter = BuildListPrefilter(filters, includesParam == "")
	}

	if paginateFirst {
		req.offset, req.limit = offset, limit
	}

	wanted := 0
	if !paginateFirst && limit > 0 {
		wanted = max(offset, 0) + limit
		req.batchSize = wanted
	}

	out := make([]map[string]any, 0)
	scanEntities(req, func(batch []map[string]any) bool {
		if includesParam != "" {
			batch = ApplyIncludesWithGuard(batch, includesParam, guard)
		}

		for _, e := range batch {
			if search == "" && api_storage.FiltersMatchEntity(e, filters) {
				out = append(out, e)
			}

			if search != "" && api_storage.SearchMatchesEntity(e, search) {
				out = append(out, e)
			}
		}

		return wanted == 0 || len(out) < wanted
	})

	if !paginateFirst {
		return applyOffsetLimit(out, offset, limit)
	}

	return out
}

func applyOffsetLimit(in []map[string]any, offset, limit int) []map[string]any {
	start := max(0, min(offset, len(in)))

	end := len(in)
	if limit > 0 && start+limit < end {
		end = start + limit
	}

	return in[start:end]
}

func scanEntities(req scanRequest, visit func([]map[string]any) bool) {
	if IsReservedCollection(req.entity) {
		return
	}

	if req.sortField != "" && sortTraversesReferences(req.entity, req.sortField) {
		scanSortedInMemory(req, visit)
		return
	}

	cur, err := openScanCursor(req)
	if err != nil && len(req.filter) > 0 {
		req.filter = bson.M{}
		cur, err = openScanCursor(req)
	}

	if err != nil && req.sortField != "" {
		log.Error(fmt.Sprintf("Sorting %s by %s in MongoDB failed, sorting in memory: %v", req.entity, req.sortField, err))
		scanSortedInMemory(req, visit)
		return
	}

	if err != nil || cur == nil {
		if err != nil {
			log.Error(fmt.Sprintf("Reading %s from MongoDB failed: %v", req.entity, err))
		}
		return
	}

	ctx := context.Background()
	defer cur.Close(ctx)

	batchSize := initialBatchSize(req.batchSize)
	cur.SetBatchSize(int32(batchSize))

	batch := make([]map[string]any, 0, batchSize)
	for cur.Next(ctx) {
		var raw map[string]any
		if cur.Decode(&raw) != nil {
			continue
		}

		batch = append(batch, NormalizeMongoDocument(raw))

		if len(batch) == batchSize {
			if !visit(batch) {
				return
			}

			batchSize = min(batchSize*2, maxScanBatchSize)
			cur.SetBatchSize(int32(batchSize))
			batch = make([]map[string]any, 0, batchSize)
		}
	}

	if err := cur.Err(); err != nil {
		log.Error(fmt.Sprintf("Reading %s from MongoDB stopped early: %v", req.entity, err))
	}

	if len(batch) > 0 {
		visit(batch)
	}
}

func initialBatchSize(hint int) int {
	if hint <= 0 {
		return scanBatchSize
	}

	return min(max(hint, minScanBatchSize), maxScanBatchSize)
}

func openScanCursor(req scanRequest) (*mongo.Cursor, error) {
	ctx := context.Background()
	collection := globals.MongoDB.Collection(req.entity)

	if req.sortField == "" {
		opts := options.Find().
			SetSort(bson.D{{Key: "$natural", Value: 1}}).
			SetBatchSize(int32(initialBatchSize(req.batchSize)))
		if req.offset > 0 {
			opts = opts.SetSkip(int64(req.offset))
		}
		if req.limit > 0 {
			opts = opts.SetLimit(int64(req.limit))
		}

		return collection.Find(ctx, req.filter, opts)
	}

	path, ok := MongoFieldPath(req.sortField)
	if !ok {
		path = ""
	}

	direction := 1
	if !req.sortAscending {
		direction = -1
	}

	pipeline := mongo.Pipeline{
		{{Key: "$match", Value: req.filter}},
		{{Key: "$addFields", Value: bson.D{{Key: sortKeyField, Value: sortKeyExpression(path)}}}},
		{{Key: "$sort", Value: bson.D{{Key: sortKeyField, Value: direction}, {Key: "_id", Value: 1}}}},
	}

	if req.offset > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$skip", Value: int64(req.offset)}})
	}
	if req.limit > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$limit", Value: int64(req.limit)}})
	}

	pipeline = append(pipeline, bson.D{{Key: "$project", Value: bson.D{{Key: sortKeyField, Value: 0}}}})

	return collection.Aggregate(ctx, pipeline, options.Aggregate().
		SetAllowDiskUse(true).
		SetBatchSize(int32(initialBatchSize(req.batchSize))))
}

func sortKeyExpression(path string) any {
	if path == "" {
		return nil
	}

	return bson.D{{Key: "$let", Value: bson.D{
		{Key: "vars", Value: bson.D{{Key: "v", Value: sortPathValue(path)}}},
		{Key: "in", Value: bson.D{{Key: "$switch", Value: bson.D{
			{Key: "branches", Value: bson.A{
				bson.D{{Key: "case", Value: hasType("$$v", "string")}, {Key: "then", Value: dateOrString("$$v")}},
				bson.D{{Key: "case", Value: hasType("$$v", "object")}, {Key: "then", Value: bson.D{{Key: "$literal", Value: bson.D{}}}}},
				bson.D{{Key: "case", Value: hasType("$$v", "array")}, {Key: "then", Value: bson.D{{Key: "$map", Value: bson.D{
					{Key: "input", Value: "$$v"},
					{Key: "as", Value: "e"},
					{Key: "in", Value: arrayElementSortKey("$$e")},
				}}}}},
			}},
			{Key: "default", Value: "$$v"},
		}}}},
	}}}
}

func sortPathValue(path string) any {
	segments := strings.Split(path, ".")
	if len(segments) == 1 {
		return "$" + path
	}

	parentsAreObjects := make(bson.A, 0, len(segments)-1)
	for i := 1; i < len(segments); i++ {
		parentsAreObjects = append(parentsAreObjects, hasType("$"+strings.Join(segments[:i], "."), "object"))
	}

	return bson.D{{Key: "$cond", Value: bson.A{bson.D{{Key: "$and", Value: parentsAreObjects}}, "$" + path, nil}}}
}

func dateOrString(value string) bson.D {
	return bson.D{{Key: "$cond", Value: bson.A{
		bson.D{{Key: "$regexMatch", Value: bson.D{{Key: "input", Value: value}, {Key: "regex", Value: datePattern}}}},
		bson.D{{Key: "$convert", Value: bson.D{
			{Key: "input", Value: bson.D{{Key: "$replaceOne", Value: bson.D{
				{Key: "input", Value: value},
				{Key: "find", Value: ","},
				{Key: "replacement", Value: "."},
			}}}},
			{Key: "to", Value: "date"},
			{Key: "onError", Value: value},
		}}},
		value,
	}}}
}

func arrayElementSortKey(element string) bson.D {
	return bson.D{{Key: "$switch", Value: bson.D{
		{Key: "branches", Value: bson.A{
			bson.D{{Key: "case", Value: hasType(element, "object")}, {Key: "then", Value: bson.D{{Key: "$literal", Value: bson.D{}}}}},
			bson.D{{Key: "case", Value: hasType(element, "array")}, {Key: "then", Value: bson.D{{Key: "$literal", Value: bson.A{0}}}}},
		}},
		{Key: "default", Value: element},
	}}}
}

func hasType(value, bsonType string) bson.D {
	return bson.D{{Key: "$eq", Value: bson.A{bson.D{{Key: "$type", Value: value}}, bsonType}}}
}

func sortTraversesReferences(entity, sortField string) bool {
	path, ok := MongoFieldPath(sortField)
	if !ok {
		return false
	}

	segments := strings.Split(path, ".")
	if len(segments) < 2 {
		return false
	}

	conditions := make(bson.A, 0, len(segments)-1)
	for i := 1; i < len(segments); i++ {
		prefix := strings.Join(segments[:i], ".")
		conditions = append(conditions, bson.M{prefix + ".@entity": bson.M{"$exists": true}})
	}

	ctx := context.Background()
	err := globals.MongoDB.Collection(entity).
		FindOne(ctx, bson.M{"$or": conditions}, options.FindOne().SetProjection(bson.M{"_id": 1})).
		Err()

	return err == nil
}

func scanSortedInMemory(req scanRequest, visit func([]map[string]any) bool) {
	all := make([]map[string]any, 0)
	scanEntities(scanRequest{entity: req.entity, filter: req.filter}, func(batch []map[string]any) bool {
		all = append(all, batch...)
		return true
	})

	keyed := make([]map[string]any, len(all))
	for i, doc := range all {
		keyed[i] = maps.Clone(doc)
	}
	ApplyIncludes(keyed, "all")

	sorted := make([]map[string]any, 0, len(all))
	for _, i := range api_storage.SortEntitiesOrder(keyed, req.sortField, req.sortAscending) {
		sorted = append(sorted, all[i])
	}

	sorted = applyOffsetLimit(sorted, req.offset, req.limit)

	for start := 0; start < len(sorted); start += scanBatchSize {
		end := min(start+scanBatchSize, len(sorted))
		if !visit(sorted[start:end]) {
			return
		}
	}
}
