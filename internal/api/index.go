package api_storage

import (
	"bytes"
	"sort"

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

func EnsureFieldIndex(entity, field, id string, value interface{}) {
	ascKey := globals.ApiEntityIndexFieldSortAscKey(entity, field)
	descKey := globals.ApiEntityIndexFieldSortDescKey(entity, field)
	rawAsc, _ := storage.GetByKey(ascKey)
	rawDesc, _ := storage.GetByKey(descKey)
	asc := decodeIDs(rawAsc)
	desc := decodeIDs(rawDesc)
	insertAsc(&asc, id)
	insertDesc(&desc, id)
	storage.PutKeyValue(ascKey, encodeIDs(asc))
	storage.PutKeyValue(descKey, encodeIDs(desc))
	AddFieldToIndexedFields(entity, field)
}

func insertAsc(ids *[]string, id string) {
	for _, v := range *ids {
		if v == id {
			return
		}
	}
	*ids = append(*ids, id)
	sort.Strings(*ids)
}

func insertDesc(ids *[]string, id string) {
	for _, v := range *ids {
		if v == id {
			return
		}
	}
	*ids = append(*ids, id)
	sort.Sort(sort.Reverse(sort.StringSlice(*ids)))
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
	for k, newVal := range newData {
		if k == "id" {
			continue
		}
		oldVal, exists := oldData[k]
		if !exists || oldVal != newVal {
			EnsureFieldIndex(entity, k, id, newVal)
		}
	}
	for k := range oldData {
		if k == "id" {
			continue
		}
		if _, exists := newData[k]; !exists {
			EnsureFieldIndex(entity, k, id, nil)
		}
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
