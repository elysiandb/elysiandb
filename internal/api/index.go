package api_storage

import (
	"bytes"
	"fmt"
	"runtime"
	"sync"

	"github.com/cespare/xxhash/v2"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func RemoveIdFromIndexes(entity string, id string) {
	idIndexKey := globals.ApiEntityIndexIdKey(entity)
	raw, _ := storage.GetByKey(idIndexKey)
	ids := decodeIDs(raw)
	changed := false
	newIds := newIdsWithout(ids, id, &changed)
	if changed {
		storage.PutKeyValue(idIndexKey, encodeIDs(newIds))
	}

	go RemoveIdFromNonMasterIndexes(entity, id)
}

func RemoveIdFromNonMasterIndexes(entity string, id string) {
	fields := GetListForIndexedFields(entity)
	for _, field := range fields {
		for _, sortKey := range []string{
			globals.ApiEntityIndexFieldSortAscKey(entity, field),
			globals.ApiEntityIndexFieldSortDescKey(entity, field),
		} {
			raw, _ := storage.GetByKey(sortKey)
			if len(raw) == 0 {
				continue
			}
			ids := decodeIDs(raw)
			changed := false
			newIds := newIdsWithout(ids, id, &changed)
			if changed {
				storage.PutKeyValue(sortKey, encodeIDs(newIds))
			}
		}
	}
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

	type indexJob struct {
		key string
		asc bool
	}

	jobs := []indexJob{
		{key: ascKey, asc: true},
		{key: descKey, asc: false},
	}

	sem := make(chan struct{}, runtime.NumCPU()*2)
	wg := sync.WaitGroup{}

	for _, job := range jobs {
		wg.Add(1)
		sem <- struct{}{}
		go func(j indexJob) {
			defer wg.Done()
			defer func() { <-sem }()
			ids := GetSortedEntityIdsByField(entity, field, j.asc)
			storage.PutKeyValue(j.key, encodeIDs(ids))
		}(job)
	}
	wg.Wait()

	AddFieldToIndexedFields(entity, field)

	if m, ok := value.(map[string]interface{}); ok {
		wg := sync.WaitGroup{}
		for k, v := range m {
			wg.Add(1)
			sem <- struct{}{}
			go func(subField string, subVal interface{}) {
				defer wg.Done()
				defer func() { <-sem }()
				nestedField := field + "." + subField
				EnsureFieldIndex(entity, nestedField, id, subVal)
			}(k, v)
		}
		wg.Wait()
	}
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
	safeKey := func(v interface{}) string {
		switch val := v.(type) {
		case []interface{}:
			h := xxhash.Sum64String(fmt.Sprint(val))
			return fmt.Sprintf("%x", h)
		case map[string]interface{}:
			h := xxhash.Sum64String(fmt.Sprint(val))
			return fmt.Sprintf("%x", h)
		default:
			return fmt.Sprintf("%v", val)
		}
	}

	sem := make(chan struct{}, runtime.NumCPU()*2)
	wg := sync.WaitGroup{}

	for k, v := range newData {
		if k == "id" {
			continue
		}
		wg.Add(1)
		sem <- struct{}{}
		go func(field string, val interface{}) {
			defer wg.Done()
			defer func() { <-sem }()
			EnsureFieldIndex(entity, field, id, val)
		}(k, v)
	}
	wg.Wait()

	wg = sync.WaitGroup{}
	for k, newVal := range newData {
		if k == "id" {
			continue
		}
		oldVal, exists := oldData[k]
		if !exists || safeKey(oldVal) != safeKey(newVal) {
			wg.Add(1)
			sem <- struct{}{}
			go func(field string, val interface{}) {
				defer wg.Done()
				defer func() { <-sem }()
				EnsureFieldIndex(entity, field, id, val)
			}(k, newVal)
		}
	}
	wg.Wait()

	wg = sync.WaitGroup{}
	for k := range oldData {
		if k == "id" {
			continue
		}
		if _, exists := newData[k]; !exists {
			wg.Add(1)
			sem <- struct{}{}
			go func(field string) {
				defer wg.Done()
				defer func() { <-sem }()
				DeleteIndexesForField(entity, field)
			}(k)
		}
	}
	wg.Wait()
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
