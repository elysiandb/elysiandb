package api_storage

import (
	"fmt"

	"github.com/google/uuid"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/storage"
)

func WriteEntity(entity string, data map[string]interface{}) []schema.ValidationError {
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		errors := schema.ValidateEntity(entity, data)
		if len(errors) > 0 {
			return errors
		}
	}

	if _, ok := data["id"].(string); !ok || data["id"] == "" {
		data["id"] = uuid.New().String()
	}

	subs := ExtractSubEntities(entity, data)
	for _, sub := range subs {
		subEntity := sub["@entity"].(string)
		delete(sub, "@entity")
		WriteEntity(subEntity, sub)
	}

	persistEntity(entity, data)
	updateSchemaIfNeeded(entity, data)

	return []schema.ValidationError{}
}

func persistEntity(entity string, data map[string]interface{}) {
	id, _ := data["id"].(string)
	key := globals.ApiSingleEntityKey(entity, id)
	old := ReadEntityById(entity, id)
	storage.PutJsonValue(key, data)
	AddIdToindexes(entity, id)
	AddEntityType(entity)

	if old != nil {
		UpdateIndexesForEntity(entity, id, old, data)
	} else {
		for k := range data {
			if k != "id" {
				markFieldAndNestedDirty(entity, k, data[k])
			}
		}
	}
}

func updateSchemaIfNeeded(entity string, data map[string]interface{}) {
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		analyzed := schema.AnalyzeEntitySchema(entity, data)
		WriteEntity(schema.SchemaEntity, analyzed)
	}
}

func WriteListOfEntities(entity string, list []map[string]interface{}) [][]schema.ValidationError {
	errors := [][]schema.ValidationError{}
	for _, data := range list {
		errors = append(errors, WriteEntity(entity, data))
	}

	return errors
}

func AddEntityType(entity string) {
	key := globals.ApiAllEntityTypesListKey()
	data, _ := storage.GetByKey(key)
	types := decodeIDs(data)
	found := false
	for _, t := range types {
		if t == entity {
			found = true
			break
		}
	}
	if !found {
		types = append(types, entity)
		storage.PutKeyValue(key, encodeIDs(types))
	}
}

func EntityTypeExists(entity string) bool {
	for _, t := range ListEntityTypes() {
		if t == entity {
			return true
		}
	}

	return false
}

func ListEntityTypes() []string {
	data, _ := storage.GetByKey(globals.ApiAllEntityTypesListKey())
	return decodeIDs(data)
}

func ReadEntityById(entity string, id string) map[string]interface{} {
	key := globals.ApiSingleEntityKey(entity, id)
	data, _ := storage.GetJsonByKey(key)
	return data
}

func ReadEntityRawById(entity string, id string) ([]byte, bool) {
	key := globals.ApiSingleEntityKey(entity, id)
	data, err := storage.GetJsonRaw(key)
	if err != nil || data == nil {
		return nil, false
	}

	return data, true
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
) []map[string]interface{} {
	idList, err := GetListOfIds(entity, sortField, sortAscending)
	if err != nil {
		return []map[string]interface{}{}
	}

	ids := decodeIDs(idList)
	if len(ids) == 0 {
		return []map[string]interface{}{}
	}

	if len(filters) == 0 {
		ids = applyOffsetLimit(ids, offset, limit)
	}

	all := make([]map[string]interface{}, 0, len(ids))
	for _, id := range ids {
		entityData := ReadEntityById(entity, id)
		if entityData != nil {
			all = append(all, entityData)
		}
	}

	if includesParam != "" {
		all = ApplyIncludes(all, includesParam)
	}

	filtered := make([]map[string]interface{}, 0, len(all))
	for _, e := range all {
		if search == "" && FiltersMatchEntity(e, filters) {
			filtered = append(filtered, e)
		}

		if search != "" && SearchMatchesEntity(e, search) {
			filtered = append(filtered, e)
		}
	}

	if len(filters) > 0 {
		return applyOffsetLimit(filtered, offset, limit)
	}

	return filtered
}

func applyOffsetLimit[T any](in []T, offset, limit int) []T {
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

func GetListOfIds(entity string, sortField string, sortAscending bool) ([]byte, error) {
	if sortField == "" {
		idIndexKey := globals.ApiEntityIndexIdKey(entity)
		return storage.GetByKey(idIndexKey)
	}
	ensureFieldIndexFresh(entity, sortField)
	if !IndexExistsForField(entity, sortField) {
		return []byte{}, nil
	}
	if sortAscending {
		return storage.GetByKey(
			globals.ApiEntityIndexFieldSortAscKey(entity, sortField),
		)
	}
	return storage.GetByKey(
		globals.ApiEntityIndexFieldSortDescKey(entity, sortField),
	)
}

func DeleteEntityById(entity string, id string) {
	key := globals.ApiSingleEntityKey(entity, id)
	storage.DeleteJsonByKey(key)
	RemoveIdFromIndexes(entity, id)
}

func DeleteAllEntities(entity string) {
	prefix := globals.ApiEntitiesAllKey(entity)
	storage.DeleteJsonByPrefix(prefix)
	RemoveEntityIndexes(entity)
}

func UpdateEntityById(entity string, id string, updated map[string]interface{}) map[string]interface{} {
	existing := ReadEntityById(entity, id)
	if existing == nil {
		return nil
	}
	old := make(map[string]interface{}, len(existing))
	for k, v := range existing {
		old[k] = v
	}
	for k, v := range updated {
		existing[k] = v
	}

	if _, ok := existing["id"].(string); !ok || existing["id"] == "" {
		existing["id"] = id
	}

	subs := ExtractSubEntities(entity, existing)
	for _, sub := range subs {
		subEntity := sub["@entity"].(string)
		delete(sub, "@entity")
		WriteEntity(subEntity, sub)
	}

	persistEntity(entity, existing)
	UpdateIndexesForEntity(entity, id, old, existing)
	updateSchemaIfNeeded(entity, existing)
	return existing
}

func UpdateListOfEntities(entity string, updates []map[string]interface{}) []map[string]interface{} {
	results := make([]map[string]interface{}, 0, len(updates))
	for _, upd := range updates {
		id, ok := upd["id"].(string)
		if !ok || id == "" {
			continue
		}
		res := UpdateEntityById(entity, id, upd)
		if res != nil {
			results = append(results, res)
		}
	}
	return results
}

func DumpAll() map[string]interface{} {
	entities := ListEntityTypes()

	result := make(map[string]interface{})
	for _, entity := range entities {
		if entity == schema.SchemaEntity {
			continue
		}

		result[entity] = ListEntities(entity, 0, 0, "", true, nil, "", "")
	}

	return result
}

func ImportAll(data map[string][]map[string]interface{}) {
	for entity, items := range data {
		storage.DeleteByWildcardKey(globals.ApiEntityIndexPatternKey(entity))

		for _, item := range items {
			if errs := WriteEntity(entity, item); len(errs) > 0 {
				log.Error(fmt.Sprintf("Error importing entity %s: %+v", entity, errs))
			}
		}

		log.Info(fmt.Sprintf("The entity '%s' has been imported", entity))
	}

	log.Info("All entities have been imported")
}
