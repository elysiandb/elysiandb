package mongodb

import (
	"context"
	"errors"
	"fmt"
	"maps"
	"slices"
	"strings"
	"time"

	"github.com/google/uuid"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/schema"
	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

func FilterFields(data map[string]any, fields []string) map[string]any {
	return api_storage.FilterFields(data, fields)
}

func WriteEntity(entity string, data map[string]any) []schema.ValidationError {
	if IsReservedCollection(entity) {
		return []schema.ValidationError{{Message: fmt.Sprintf("entity type '%s' is reserved", entity)}}
	}

	schemaData := GetEntitySchema(entity)
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		errors := schema.ValidateEntity(entity, data, schemaData)
		if len(errors) > 0 {
			return errors
		}
	}

	if _, ok := data["id"].(string); !ok || data["id"] == "" {
		data["id"] = uuid.New().String()
	}

	writeSubEntities(entity, data)

	if err := persistEntity(entity, data); err != nil {
		return []schema.ValidationError{{Message: err.Error()}}
	}

	if !schema.IsManualSchema(entity, schemaData) || !globals.GetConfig().Api.Schema.Strict {
		UpdateSchemaIfNeeded(entity, data)
	}

	return []schema.ValidationError{}
}

func writeSubEntities(entity string, data map[string]any) {
	for _, sub := range api_storage.ExtractSubEntities(entity, data) {
		subEntity := sub["@entity"].(string)
		delete(sub, "@entity")
		WriteEntity(subEntity, sub)
	}
}

func persistEntity(entity string, data map[string]any) error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	doc := ToMongoDocument(data)
	_, err := globals.MongoDB.Collection(entity).ReplaceOne(
		ctx,
		bson.M{"_id": doc["_id"]},
		doc,
		options.Replace().SetUpsert(true),
	)

	return err
}

func UpdateSchemaIfNeeded(entity string, data map[string]any) {
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		analyzed := schema.AnalyzeEntitySchema(entity, data)
		WriteEntity(schema.SchemaEntity, analyzed)
	}
}

func UpdateEntitySchema(entity string, fieldsRaw map[string]any) map[string]any {
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
	if IsReservedCollection(entity) {
		return fmt.Errorf("entity type '%s' is reserved", entity)
	}

	if EntityTypeExists(entity) {
		return fmt.Errorf("entity type '%s' already exists", entity)
	}

	return createCollection(entity)
}

func DeleteEntityType(entity string) error {
	if !EntityTypeExists(entity) {
		return fmt.Errorf("entity type '%s' does not exist", entity)
	}

	return globals.MongoDB.Collection(entity).Drop(context.Background())
}

func WriteListOfEntities(entity string, list []map[string]any) [][]schema.ValidationError {
	out := make([][]schema.ValidationError, 0, len(list))
	for _, d := range list {
		out = append(out, WriteEntity(entity, d))
	}

	return out
}

func AddEntityType(entity string) {
	if !IsReservedCollection(entity) && !EntityTypeExists(entity) {
		_ = createCollection(entity)
	}
}

func createCollection(entity string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	err := globals.MongoDB.CreateCollection(ctx, entity)

	var cmdErr mongo.CommandError
	if errors.As(err, &cmdErr) && cmdErr.Name == "NamespaceExists" {
		return nil
	}

	return err
}

func GetEntitySchema(entity string) map[string]any {
	return ReadEntityById(schema.SchemaEntity, entity)
}

func EntityTypeExists(entity string) bool {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if IsReservedCollection(entity) {
		return false
	}

	names, err := globals.MongoDB.ListCollectionNames(ctx, bson.M{"name": entity, "type": "collection"})

	return err == nil && len(names) > 0
}

func ListEntityTypes() []string {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	names, err := globals.MongoDB.ListCollectionNames(ctx, bson.M{"type": "collection"})
	if err != nil {
		return []string{}
	}

	types := make([]string, 0, len(names))
	for _, name := range names {
		if !strings.HasPrefix(name, "system.") && name != SettingsCollection {
			types = append(types, name)
		}
	}

	slices.Sort(types)

	return types
}

func ListPublicEntityTypes() []string {
	all := ListEntityTypes()

	public := make([]string, 0, len(all))
	for _, entityType := range all {
		if !strings.HasPrefix(entityType, api_storage.CoreEntityTypePrefix) {
			public = append(public, entityType)
		}
	}

	return public
}

func ReadEntityById(entity, id string) map[string]any {
	if IsReservedCollection(entity) {
		return nil
	}

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

func DeleteEntityById(entity, id string) {
	if IsReservedCollection(entity) {
		return
	}

	ctx := context.Background()
	_, _ = globals.MongoDB.Collection(entity).DeleteOne(ctx, bson.M{"_id": id})
}

func DeleteAllEntities(entity string) {
	if IsReservedCollection(entity) {
		return
	}

	_ = globals.MongoDB.Collection(entity).Drop(context.Background())
}

func DeleteAll() {
	for _, entity := range ListEntityTypes() {
		DeleteAllEntities(entity)
	}
}

func UpdateEntityById(entity, id string, updated map[string]any) map[string]any {
	existing := ReadEntityById(entity, id)
	if existing == nil {
		return nil
	}

	maps.Copy(existing, updated)

	if _, ok := existing["id"].(string); !ok || existing["id"] == "" {
		existing["id"] = id
	}

	writeSubEntities(entity, existing)

	if err := persistEntity(entity, existing); err != nil {
		log.Error(fmt.Sprintf("Error updating entity %s/%s: %v", entity, id, err))
	}

	if !schema.IsManualSchema(entity, GetEntitySchema(entity)) || !globals.GetConfig().Api.Schema.Strict {
		UpdateSchemaIfNeeded(entity, existing)
	}

	return existing
}

func UpdateListOfEntities(entity string, updates []map[string]any) []map[string]any {
	results := make([]map[string]any, 0, len(updates))
	for _, upd := range updates {
		id, ok := upd["id"].(string)
		if !ok || id == "" {
			continue
		}

		if res := UpdateEntityById(entity, id, upd); res != nil {
			results = append(results, res)
		}
	}

	return results
}

func DumpAll() map[string]any {
	out := map[string]any{}
	for _, entity := range ListEntityTypes() {
		if entity == schema.SchemaEntity {
			continue
		}

		out[entity] = ListEntities(entity, 0, 0, "", true, nil, "", "")
	}

	return out
}

func EntityExists(entity, id string) bool {
	if IsReservedCollection(entity) {
		return false
	}

	ctx := context.Background()
	n, _ := globals.MongoDB.Collection(entity).CountDocuments(ctx, bson.M{"_id": id}, options.Count().SetLimit(1))

	return n > 0
}

func CountAllEntities() int {
	total := 0
	for _, entity := range ListEntityTypes() {
		if entity == schema.SchemaEntity {
			continue
		}

		n, _ := globals.MongoDB.Collection(entity).CountDocuments(context.Background(), bson.M{})
		total += int(n)
	}

	return total
}

func ImportAll(data map[string][]map[string]any) {
	for entity, items := range data {
		for _, item := range items {
			if errs := WriteEntity(entity, item); len(errs) > 0 {
				log.Error(fmt.Sprintf("Error importing entity %s: %+v", entity, errs))
			}
		}

		log.Info(fmt.Sprintf("The entity '%s' has been imported", entity))
	}

	log.Info("All entities have been imported")
}

func ExecuteMigrations(migrationQueries []api_storage.MigrationQuery) error {
	return api_storage.ApplyMigrations(migrationQueries, ListEntities, ReadEntityById, ApplyIncludes, UpdateEntityById)
}
