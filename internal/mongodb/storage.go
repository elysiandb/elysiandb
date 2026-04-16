package mongodb

import (
	"bytes"
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

type IncludeSpec struct {
	Path []string
	From string
	As   string
	Tmp  string
}

type RefLoc struct {
	ParentMap map[string]any
	Key       string
	ParentArr []any
	Idx       int
}

func FilterFields(data map[string]any, fields []string) map[string]any {
	if len(fields) == 0 {
		return data
	}

	return api_storage.FilterFields(data, fields)
}

func FindOptions(limit int, offset int, sortField string, sortAscending bool) options.Lister[options.FindOptions] {
	opts := options.Find()

	if limit > 0 {
		opts = opts.SetLimit(int64(limit))
	}
	if offset > 0 {
		opts = opts.SetSkip(int64(offset))
	}
	if sortField != "" {
		dir := 1
		if !sortAscending {
			dir = -1
		}
		opts = opts.SetSort(bson.D{{Key: sortField, Value: dir}})
	}

	return opts
}

func ResolveIncludesAllRecursive(items []map[string]any, maxDepth int) {
	if len(items) == 0 || maxDepth <= 0 {
		return
	}

	for depth := 0; depth < maxDepth; depth++ {
		refs := make([]struct {
			Loc    RefLoc
			Entity string
			Id     string
		}, 0)

		for _, it := range items {
			CollectAllRefMaps(it, &refs)
		}

		if len(refs) == 0 {
			return
		}

		byEntity := map[string]map[string]struct{}{}
		for _, r := range refs {
			m, ok := byEntity[r.Entity]
			if !ok {
				m = map[string]struct{}{}
				byEntity[r.Entity] = m
			}
			m[r.Id] = struct{}{}
		}

		loaded := map[string]map[string]map[string]any{}
		for ent, set := range byEntity {
			ids := make([]string, 0, len(set))
			for id := range set {
				ids = append(ids, id)
			}
			loaded[ent] = LoadDocsByIds(ent, ids)
		}

		ApplyLoadedRefs(refs, loaded)
	}
}

func WriteEntity(entity string, data map[string]interface{}) []schema.ValidationError {
	schemaData := GetEntitySchema(entity)
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		errors := schema.ValidateEntity(entity, data, schemaData)
		if len(errors) > 0 {
			return errors
		}
	}

	if !EntityTypeExists(entity) {
		AddEntityType(entity)
	}

	if _, ok := data["id"].(string); !ok || data["id"] == "" {
		data["id"] = uuid.New().String()
	}

	subs := api_storage.ExtractSubEntities(entity, data)
	for _, sub := range subs {
		subEntity := sub["@entity"].(string)
		delete(sub, "@entity")
		WriteEntity(subEntity, sub)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	doc := ToMongoDocument(data)

	_, err := globals.MongoDB.Collection(entity).ReplaceOne(
		ctx,
		bson.M{"_id": doc["_id"]},
		doc,
		options.Replace().SetUpsert(true),
	)
	if err != nil {
		return []schema.ValidationError{{Message: err.Error()}}
	}

	if !schema.IsManualSchema(entity, schemaData) || !globals.GetConfig().Api.Schema.Strict {
		UpdateSchemaIfNeeded(entity, data)
	}

	return []schema.ValidationError{}
}

func UpdateSchemaIfNeeded(entity string, data map[string]interface{}) {
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		analyzed := schema.AnalyzeEntitySchema(entity, data)
		WriteEntity(schema.SchemaEntity, analyzed)
	}
}

func UpdateEntitySchema(entity string, fieldsRaw map[string]interface{}) map[string]interface{} {
	fields := schema.MapToFields(fieldsRaw)

	entitySchema := schema.Entity{
		ID:     entity,
		Fields: fields,
	}

	storable := schema.SchemaEntityToStorableStructure(entitySchema)
	storable["_manual"] = true

	WriteEntity(schema.SchemaEntity, storable)

	return storable
}

func CreateEntityType(entity string) error {
	if EntityTypeExists(entity) {
		return fmt.Errorf("entity type '%s' already exists", entity)
	}

	AddEntityType(entity)
	return nil
}

func DeleteEntityType(entity string) error {
	ctx := context.Background()
	return globals.MongoDB.Collection(entity).Drop(ctx)
}

func WriteListOfEntities(entity string, list []map[string]interface{}) [][]schema.ValidationError {
	out := make([][]schema.ValidationError, 0, len(list))
	for _, d := range list {
		out = append(out, WriteEntity(entity, d))
	}
	return out
}

func AddEntityType(entity string) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	names, _ := globals.MongoDB.ListCollectionNames(ctx, bson.M{"name": entity})
	if len(names) == 0 {
		_ = globals.MongoDB.CreateCollection(ctx, entity)
	}
}

func GetEntitySchema(entity string) map[string]interface{} {
	return ReadEntityById(schema.SchemaEntity, entity)
}

func EntityTypeExists(entity string) bool {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	names, err := globals.MongoDB.ListCollectionNames(ctx, bson.M{"name": entity})
	if err != nil {
		return false
	}

	return len(names) > 0
}

func ListEntityTypes() []string {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	names, err := globals.MongoDB.ListCollectionNames(ctx, bson.M{})
	if err != nil {
		return nil
	}

	return names
}

func ListPublicEntityTypes() []string {
	all := ListEntityTypes()
	out := make([]string, 0, len(all))
	for _, name := range all {
		if !strings.HasPrefix(name, api_storage.CoreEntityTypePrefix) {
			out = append(out, name)
		}
	}

	return out
}

func ReadEntityById(entity, id string) map[string]any {
	ctx := context.Background()

	raw := map[string]any{}
	if globals.MongoDB.
		Collection(entity).
		FindOne(ctx, bson.M{"_id": id}).
		Decode(&raw) != nil {
		return nil
	}

	return NormalizeMongoDocument(raw)
}

func ApplyFiltersToList(entities []map[string]any, filters map[string]map[string]string) []map[string]any {
	return api_storage.ApplyFiltersToList(entities, filters)
}

func GetListOfIds(entity, sortField string, sortAscending bool) ([]byte, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	findOpts := options.Find().SetProjection(bson.M{"_id": 1})
	if sortField != "" {
		dir := 1
		if !sortAscending {
			dir = -1
		}
		findOpts = findOpts.SetSort(bson.D{{Key: sortField, Value: dir}})
	}

	cur, err := globals.MongoDB.Collection(entity).Find(ctx, bson.M{}, findOpts)
	if err != nil {
		return nil, err
	}
	defer cur.Close(ctx)

	var ids []string
	for cur.Next(ctx) {
		var doc struct {
			ID string `bson:"_id"`
		}
		if cur.Decode(&doc) == nil && doc.ID != "" {
			ids = append(ids, doc.ID)
		}
	}

	if len(ids) == 0 {
		return nil, nil
	}

	encoded := make([][]byte, 0, len(ids))
	for _, id := range ids {
		encoded = append(encoded, []byte(id))
	}

	return bytes.Join(encoded, []byte{'\n'}), nil
}

func DeleteEntityById(entity, id string) {
	ctx := context.Background()
	globals.MongoDB.Collection(entity).DeleteOne(ctx, bson.M{"_id": id})
}

func DeleteAllEntities(entity string) {
	ctx := context.Background()
	globals.MongoDB.Collection(entity).DeleteMany(ctx, bson.M{})
	DeleteEntityType(entity)
}

func DeleteAll() {
	for _, e := range ListEntityTypes() {
		DeleteAllEntities(e)
	}
}

func UpdateEntityById(entity, id string, updated map[string]interface{}) map[string]interface{} {
	ctx := context.Background()
	update := ToMongoDocument(updated)
	globals.MongoDB.Collection(entity).UpdateOne(ctx, bson.M{"_id": id}, bson.M{"$set": update})
	return ReadEntityById(entity, id)
}

func UpdateListOfEntities(entity string, updates []map[string]interface{}) []map[string]interface{} {
	out := make([]map[string]interface{}, 0, len(updates))
	for _, u := range updates {
		if id, ok := u["id"].(string); ok {
			out = append(out, UpdateEntityById(entity, id, u))
		}
	}
	return out
}

func DumpAll() map[string]interface{} {
	out := map[string]interface{}{}
	for _, e := range ListEntityTypes() {
		out[e] = ListEntities(e, 0, 0, "", true, nil, "", "")
	}
	return out
}

func EntityExists(entity, id string) bool {
	ctx := context.Background()
	n, _ := globals.MongoDB.Collection(entity).CountDocuments(ctx, bson.M{"_id": id})
	return n > 0
}

func CountAllEntities() int {
	total := 0
	for _, e := range ListEntityTypes() {
		ctx := context.Background()
		n, _ := globals.MongoDB.Collection(e).CountDocuments(ctx, bson.M{})
		total += int(n)
	}
	return total
}

func ImportAll(data map[string][]map[string]interface{}) {
	for e, list := range data {
		for _, d := range list {
			WriteEntity(e, d)
		}
	}
}
