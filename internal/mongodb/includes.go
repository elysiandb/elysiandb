package mongodb

import (
	"context"
	"maps"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"go.mongodb.org/mongo-driver/v2/bson"
)

const (
	includePrefetchMaxRounds = 32
	includePrefetchBatchSize = 500
)

type entityRef struct {
	entity string
	id     string
}

type includeCache struct {
	docs map[entityRef]map[string]any
}

func ApplyIncludes(data []map[string]any, includesParam string) []map[string]any {
	return ApplyIncludesWithGuard(data, includesParam, nil)
}

func ApplyIncludesWithGuard(data []map[string]any, includesParam string, guard api_storage.ReadGuard) []map[string]any {
	cache := &includeCache{docs: map[entityRef]map[string]any{}}
	cache.prefetch(data, includesParam, guard)

	return api_storage.ApplyIncludes(data, includesParam, guard.Apply(cache.read))
}

func (c *includeCache) read(entity, id string) map[string]any {
	key := entityRef{entity: entity, id: id}
	doc, ok := c.docs[key]
	if !ok {
		doc = ReadEntityById(entity, id)
		c.docs[key] = doc
	}

	return cloneDocument(doc)
}

func (c *includeCache) prefetch(data []map[string]any, includesParam string, guard api_storage.ReadGuard) {
	for round := 0; round < includePrefetchMaxRounds; round++ {
		missing := map[string][]string{}
		requested := map[entityRef]struct{}{}

		probe := func(entity, id string) map[string]any {
			key := entityRef{entity: entity, id: id}
			if doc, ok := c.docs[key]; ok {
				return cloneDocument(doc)
			}

			if _, ok := requested[key]; !ok {
				requested[key] = struct{}{}
				missing[entity] = append(missing[entity], id)
			}

			return nil
		}

		dryRun := make([]map[string]any, len(data))
		for i, item := range data {
			dryRun[i] = maps.Clone(item)
		}

		api_storage.ApplyIncludes(dryRun, includesParam, guard.Apply(probe))

		if len(missing) == 0 {
			return
		}

		for entity, ids := range missing {
			c.load(entity, ids)
		}
	}
}

func (c *includeCache) load(entity string, ids []string) {
	for start := 0; start < len(ids); start += includePrefetchBatchSize {
		end := min(start+includePrefetchBatchSize, len(ids))
		chunk := ids[start:end]

		for _, id := range chunk {
			c.docs[entityRef{entity: entity, id: id}] = nil
		}

		for _, doc := range findDocumentsByIds(entity, chunk) {
			if id, ok := doc["id"].(string); ok {
				c.docs[entityRef{entity: entity, id: id}] = doc
			}
		}
	}
}

func findDocumentsByIds(entity string, ids []string) []map[string]any {
	if IsReservedCollection(entity) {
		return nil
	}

	ctx := context.Background()

	cur, err := globals.MongoDB.Collection(entity).Find(ctx, bson.M{"_id": bson.M{"$in": ids}})
	if err != nil {
		return nil
	}
	defer cur.Close(ctx)

	out := make([]map[string]any, 0, len(ids))
	for cur.Next(ctx) {
		var raw map[string]any
		if cur.Decode(&raw) == nil {
			out = append(out, NormalizeMongoDocument(raw))
		}
	}

	return out
}
