package api_storage

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func WriteEntity(entity string, data map[string]interface{}) {
	key := globals.ApiSingleEntityKey(entity, data["id"].(string))

	jsonData, _ := json.Marshal(data)
	storage.PutKeyValue(key, jsonData)
	AddIdToindexes(entity, data["id"].(string))
	UpdateIndexesForEntity(entity)
}

func ListEntities(entity string, limit int, offset int, sortField string, sortAscending bool) []map[string]interface{} {
	idList, err := GetListOfIds(entity, sortField, sortAscending)
	if err != nil {
		return []map[string]interface{}{}
	}

	var ids []string
	if idList == nil {
		return []map[string]interface{}{}
	} else {
		if err := json.Unmarshal(idList, &ids); err != nil {
			return []map[string]interface{}{}
		}
	}

	if offset > 0 && offset < len(ids) {
		ids = ids[offset:]
	} else if offset >= len(ids) {
		return []map[string]interface{}{}
	}

	if limit > 0 && limit < len(ids) {
		ids = ids[:limit]
	}

	results := []map[string]interface{}{}
	for _, id := range ids {
		entityData := ReadEntityById(entity, id)
		if entityData != nil {
			results = append(results, entityData)
		}
	}

	return results
}

func GetListOfIds(entity string, sortField string, sortAscending bool) ([]byte, error) {
	if sortField == "" {
		idIndexKey := globals.ApiEntityIndexIdKey(entity)
		return storage.GetByKey(idIndexKey)
	}

	if !IndexExistsForField(entity, sortField) {
		CreateIndexesForField(entity, sortField)
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

func ReadEntityById(entity string, id string) map[string]interface{} {
	key := globals.ApiSingleEntityKey(entity, id)
	data, _ := storage.GetByKey(key)

	var result map[string]interface{}
	if err := json.Unmarshal(data, &result); err == nil {
		return result
	}

	return nil
}

func DeleteEntityById(entity string, id string) {
	key := globals.ApiSingleEntityKey(entity, id)
	storage.DeleteByKey(key)
	RemoveIdFromIndexes(entity, id)
}

func DeleteAllEntities(entity string) {
	prefix := globals.ApiEntitiesAllKey(entity)
	storage.DeleteByWildcardKey(prefix)

	RemoveEntityIndexes(entity)
}

func UpdateEntityById(entity string, id string, updated map[string]interface{}) map[string]interface{} {
	existing := ReadEntityById(entity, id)
	if existing == nil {
		return nil
	}

	for k, v := range updated {
		existing[k] = v
	}

	WriteEntity(entity, existing)

	UpdateIndexesForEntity(entity)

	return existing
}
