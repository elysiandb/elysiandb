package api_storage

import (
	"github.com/taymour/elysiandb/internal/globals"
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

	key := globals.ApiSingleEntityKey(entity, data["id"].(string))
	old := ReadEntityById(entity, data["id"].(string))
	storage.PutJsonValue(key, data)
	AddIdToindexes(entity, data["id"].(string))
	AddEntityType(entity)
	if old != nil {
		UpdateIndexesForEntity(entity, data["id"].(string), old, data)
	} else {
		for k := range data {
			if k != "id" {
				markFieldAndNestedDirty(entity, k, data[k])
			}
		}
	}

	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		analyzed := schema.AnalyzeEntitySchema(entity, data)
		WriteEntity(schema.SchemaEntity, analyzed)
	}

	return []schema.ValidationError{}
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

func ListEntityTypes() []string {
	data, _ := storage.GetByKey(globals.ApiAllEntityTypesListKey())
	return decodeIDs(data)
}

func ReadEntityById(entity string, id string) map[string]interface{} {
	key := globals.ApiSingleEntityKey(entity, id)
	data, _ := storage.GetJsonByKey(key)
	return data
}

func ListEntities(
	entity string,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	filters map[string]map[string]string,
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
	filtered := make([]map[string]interface{}, 0, len(ids))
	for _, id := range ids {
		entityData := ReadEntityById(entity, id)
		if entityData != nil {
			if !FiltersMatchEntity(entityData, filters) {
				continue
			}
			filtered = append(filtered, entityData)
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
	WriteEntity(entity, existing)
	UpdateIndexesForEntity(entity, id, old, existing)
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
