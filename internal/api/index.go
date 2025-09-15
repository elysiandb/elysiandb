package api_storage

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func RemoveIdFromIndexes(entity string, id string) {
	idIndexKey := globals.ApiEntityIndexIdKey(entity)
	idList, _ := storage.GetByKey(idIndexKey)

	var ids []string
	if idList == nil {
		ids = []string{}
	} else {
		if err := json.Unmarshal(idList, &ids); err != nil {
			ids = []string{}
		}
	}

	newIds := []string{}
	for _, existingId := range ids {
		if existingId != id {
			newIds = append(newIds, existingId)
		}
	}
	ids = newIds
	idsBytes, _ := json.Marshal(ids)
	storage.PutKeyValue(idIndexKey, idsBytes)

	UpdateIndexesForEntity(entity)
}

func AddIdToindexes(entity string, id string) {
	idIndexKey := globals.ApiEntityIndexIdKey(entity)
	idList, _ := storage.GetByKey(idIndexKey)

	var ids []string
	if idList == nil {
		ids = []string{}
	} else {
		if err := json.Unmarshal(idList, &ids); err != nil {
			ids = []string{}
		}
	}

	ids = append(ids, id)
	idsBytes, _ := json.Marshal(ids)
	storage.PutKeyValue(idIndexKey, idsBytes)

	UpdateIndexesForEntity(entity)
}

func RemoveEntityIndexes(entity string) {
	storage.DeleteByWildcardKey(
		globals.ApiEntityIndexPatternKey(entity),
	)
}

func CreateIndexesForField(entity string, field string) {
	ascSorted, _ := json.Marshal(GetSortedEntityIdsByField(entity, field, true))
	storage.PutKeyValue(
		globals.ApiEntityIndexFieldSortAscKey(entity, field),
		ascSorted,
	)

	descSorted, _ := json.Marshal(GetSortedEntityIdsByField(entity, field, false))
	storage.PutKeyValue(
		globals.ApiEntityIndexFieldSortDescKey(entity, field),
		descSorted,
	)

	AddFieldToIndexedFields(entity, field)
}

func IndexExistsForField(entity string, field string) bool {
	ascKey := globals.ApiEntityIndexFieldSortAscKey(entity, field)
	descKey := globals.ApiEntityIndexFieldSortDescKey(entity, field)

	ascData, _ := storage.GetByKey(ascKey)
	descData, _ := storage.GetByKey(descKey)

	return ascData != nil && descData != nil
}

func GetListForIndexedFields(entity string) []string {
	fields, err := storage.GetByKey(
		globals.ApiEntityIndexAllFieldsKey(entity),
	)
	if err != nil {
		return nil
	}

	var listOfFields []string
	if fields == nil {
		listOfFields = []string{}
	} else {
		if err := json.Unmarshal(fields, &listOfFields); err != nil {
			return nil
		}
	}

	return listOfFields
}

func AddFieldToIndexedFields(entity string, field string) {
	fields, _ := storage.GetByKey(
		globals.ApiEntityIndexAllFieldsKey(entity),
	)

	var listOfFields []string
	if fields == nil {
		listOfFields = []string{}
	} else {
		if err := json.Unmarshal(fields, &listOfFields); err != nil {
			listOfFields = []string{}
		}
	}

	for _, existingField := range listOfFields {
		if existingField == field {
			return
		}
	}

	listOfFields = append(listOfFields, field)
	fieldsBytes, _ := json.Marshal(listOfFields)
	storage.PutKeyValue(
		globals.ApiEntityIndexAllFieldsKey(entity),
		fieldsBytes,
	)
}

func UpdateIndexesForEntity(entity string) {
	fields := GetListForIndexedFields(entity)
	for _, field := range fields {
		CreateIndexesForField(entity, field)
	}
}

func DeleteIndexesForField(entity string, field string) {
	storage.DeleteByWildcardKey(
		globals.ApiEntityIndexFieldAllKey(entity, field),
	)
}
