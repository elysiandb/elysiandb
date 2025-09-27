package api_storage

import (
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func WriteEntity(entity string, data map[string]interface{}) {
	key := globals.ApiSingleEntityKey(entity, data["id"].(string))
	old := ReadEntityById(entity, data["id"].(string))
	storage.PutJsonValue(key, data)
	AddIdToindexes(entity, data["id"].(string))
	if old != nil {
		UpdateIndexesForEntity(entity, data["id"].(string), old, data)
	} else {
		for k := range data {
			if k != "id" {
				EnsureFieldIndex(entity, k, data["id"].(string), data[k])
			}
		}
	}
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
