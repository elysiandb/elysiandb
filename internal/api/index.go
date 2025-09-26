package api_storage

import (
	"bytes"
	"reflect"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func RemoveIdFromIndexes(entity string, id string) {
	idIndexKey := globals.ApiEntityIndexIdKey(entity)
	raw, _ := storage.GetByKey(idIndexKey)
	ids := decodeIDs(raw)

	changed := false
	newIds := newIdsWithout(ids, id, &changed)
	if !changed {
		return
	}

	storage.PutKeyValue(idIndexKey, encodeIDs(newIds))
}

func decodeIDs(b []byte) []string {
	if len(b) == 0 {
		return nil
	}
	parts := bytes.Split(b, []byte{'\n'})
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if len(p) > 0 {
			out = append(out, string(p))
		}
	}
	return out
}

func encodeIDs(ids []string) []byte {
	if len(ids) == 0 {
		return nil
	}
	parts := make([][]byte, 0, len(ids))
	for _, id := range ids {
		parts = append(parts, []byte(id))
	}
	return bytes.Join(parts, []byte{'\n'})
}

func AddIdToindexes(entity string, id string) {
	k := globals.ApiEntityIndexIdKey(entity)
	raw, _ := storage.GetByKey(k)

	ids := decodeIDs(raw)
	for _, v := range ids {
		if v == id {
			return
		}
	}
	ids = append(ids, id)

	_ = storage.PutKeyValue(k, encodeIDs(ids))
}

func RemoveEntityIndexes(entity string) {
	storage.DeleteByWildcardKey(
		globals.ApiEntityIndexPatternKey(entity),
	)
}

func CreateIndexesForField(entity string, field string) {
	ascSorted := encodeIDs(GetSortedEntityIdsByField(entity, field, true))
	storage.PutKeyValue(
		globals.ApiEntityIndexFieldSortAscKey(entity, field),
		ascSorted,
	)

	descSorted := encodeIDs(GetSortedEntityIdsByField(entity, field, false))
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
	raw, _ := storage.GetByKey(globals.ApiEntityIndexAllFieldsKey(entity))
	return decodeIDs(raw)
}

func AddFieldToIndexedFields(entity string, field string) {
	fields, _ := storage.GetByKey(
		globals.ApiEntityIndexAllFieldsKey(entity),
	)

	listOfFields := decodeIDs(fields)
	for _, existingField := range listOfFields {
		if existingField == field {
			return
		}
	}

	listOfFields = append(listOfFields, field)
	storage.PutKeyValue(
		globals.ApiEntityIndexAllFieldsKey(entity),
		encodeIDs(listOfFields),
	)
}

func UpdateIndexesForEntity(entity string, id string, oldData, newData map[string]interface{}) {
	changedFields := make(map[string]struct{})

	for k, newVal := range newData {
		if k == "id" {
			continue
		}
		oldVal, exists := oldData[k]
		if !exists || !reflect.DeepEqual(oldVal, newVal) {
			changedFields[k] = struct{}{}
		}
	}

	for k := range oldData {
		if k == "id" {
			continue
		}
		if _, exists := newData[k]; !exists {
			changedFields[k] = struct{}{}
		}
	}

	for f := range changedFields {
		CreateIndexesForField(entity, f)
	}
}

func DeleteIndexesForField(entity string, field string) {
	storage.DeleteByWildcardKey(
		globals.ApiEntityIndexFieldAllKey(entity, field),
	)
}

func newIdsWithout(ids []string, id string, changed *bool) []string {
	if len(ids) == 0 {
		return ids
	}
	out := ids[:0]
	for _, existing := range ids {
		if existing == id {
			*changed = true
			continue
		}
		out = append(out, existing)
	}
	return out
}
