package api_test

import (
	"bytes"
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func initIdxTestStore(t *testing.T) {
	t.Helper()
	tmp := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder: tmp,
			Shards: 8,
		},
		Stats: configuration.StatsConfig{
			Enabled: false,
		},
	})
	storage.LoadDB()
	storage.LoadJsonDB()
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

func TestAddIdToindexes_And_RemoveIdFromIndexes(t *testing.T) {
	initIdxTestStore(t)

	entity := "idx_users"
	api_storage.AddIdToindexes(entity, "u1")
	api_storage.AddIdToindexes(entity, "u2")

	key := globals.ApiEntityIndexIdKey(entity)
	raw, err := storage.GetByKey(key)
	if err != nil {
		t.Fatalf("GetByKey: %v", err)
	}
	got := decodeIDs(raw)
	if !reflect.DeepEqual(got, []string{"u1", "u2"}) {
		t.Fatalf("ids=%v, want [u1 u2]", got)
	}

	api_storage.RemoveIdFromIndexes(entity, "u1")

	raw2, err := storage.GetByKey(key)
	if err != nil {
		t.Fatalf("GetByKey after remove: %v", err)
	}
	got2 := decodeIDs(raw2)
	if !reflect.DeepEqual(got2, []string{"u2"}) {
		t.Fatalf("ids=%v, want [u2]", got2)
	}
}

func TestAddFieldToIndexedFields_Dedup_And_GetList(t *testing.T) {
	initIdxTestStore(t)

	entity := "idx_articles"
	api_storage.AddFieldToIndexedFields(entity, "title")
	api_storage.AddFieldToIndexedFields(entity, "title")
	api_storage.AddFieldToIndexedFields(entity, "created_at")

	list := api_storage.GetListForIndexedFields(entity)
	if !reflect.DeepEqual(asSet(list), asSet([]string{"title", "created_at"})) {
		t.Fatalf("fields=%v", list)
	}
}

func TestEnsureFieldIndex_And_IndexExists(t *testing.T) {
	initIdxTestStore(t)

	entity := "idx_scores"
	api_storage.WriteEntity(entity, map[string]any{"id": "a", "score": 2})
	api_storage.WriteEntity(entity, map[string]any{"id": "b", "score": 1})
	api_storage.WriteEntity(entity, map[string]any{"id": "c", "score": 3})

	if !api_storage.IndexExistsForField(entity, "score") {
		t.Fatalf("index should exist")
	}

	ascKey := globals.ApiEntityIndexFieldSortAscKey(entity, "score")
	descKey := globals.ApiEntityIndexFieldSortDescKey(entity, "score")

	ascRaw, err := storage.GetByKey(ascKey)
	if err != nil {
		t.Fatalf("asc GetByKey: %v", err)
	}
	descRaw, err := storage.GetByKey(descKey)
	if err != nil {
		t.Fatalf("desc GetByKey: %v", err)
	}

	asc := decodeIDs(ascRaw)
	desc := decodeIDs(descRaw)

	if !reflect.DeepEqual(asc, []string{"a", "b", "c"}) {
		t.Fatalf("asc=%v", asc)
	}
	if !reflect.DeepEqual(desc, []string{"c", "b", "a"}) {
		t.Fatalf("desc=%v", desc)
	}

	fields := api_storage.GetListForIndexedFields(entity)
	if !reflect.DeepEqual(fields, []string{"score"}) {
		t.Fatalf("fields=%v", fields)
	}
}

func TestRemoveEntityIndexes(t *testing.T) {
	initIdxTestStore(t)

	entity := "idx_remove"
	api_storage.AddIdToindexes(entity, "x1")
	api_storage.AddFieldToIndexedFields(entity, "age")
	api_storage.EnsureFieldIndex(entity, "age", "x1", 42)

	api_storage.RemoveEntityIndexes(entity)

	keys := []string{
		globals.ApiEntityIndexIdKey(entity),
		globals.ApiEntityIndexAllFieldsKey(entity),
		globals.ApiEntityIndexFieldSortAscKey(entity, "age"),
		globals.ApiEntityIndexFieldSortDescKey(entity, "age"),
	}
	for _, k := range keys {
		if v, _ := storage.GetByKey(k); v != nil {
			t.Fatalf("expected key %q to be deleted, got %q", k, string(v))
		}
	}
}

func TestIndexesCreatedOnWriteEntity(t *testing.T) {
	initIdxTestStore(t)

	entity := "idx_update"
	api_storage.WriteEntity(entity, map[string]any{"id": "p1", "rank": 10})
	api_storage.WriteEntity(entity, map[string]any{"id": "p2", "rank": 5})

	if !api_storage.IndexExistsForField(entity, "rank") {
		t.Fatalf("expected index after WriteEntity")
	}

	ascKey := globals.ApiEntityIndexFieldSortAscKey(entity, "rank")
	ascRaw, err := storage.GetByKey(ascKey)
	if err != nil {
		t.Fatalf("asc GetByKey: %v", err)
	}
	asc := decodeIDs(ascRaw)
	if !reflect.DeepEqual(asc, []string{"p1", "p2"}) {
		t.Fatalf("asc=%v", asc)
	}
}

func TestDeleteIndexesForField(t *testing.T) {
	initIdxTestStore(t)

	entity := "idx_delete_field"
	api_storage.WriteEntity(entity, map[string]any{"id": "u1", "score": 10, "age": 30})
	api_storage.WriteEntity(entity, map[string]any{"id": "u2", "score": 5, "age": 25})

	if !api_storage.IndexExistsForField(entity, "score") || !api_storage.IndexExistsForField(entity, "age") {
		t.Fatalf("expected indexes for score and age to exist before deletion")
	}

	api_storage.DeleteIndexesForField(entity, "score")

	if api_storage.IndexExistsForField(entity, "score") {
		t.Fatalf("index for 'score' should not exist after DeleteIndexesForField")
	}
	if !api_storage.IndexExistsForField(entity, "age") {
		t.Fatalf("index for 'age' should still exist")
	}

	fields := api_storage.GetListForIndexedFields(entity)
	wantSet := asSet([]string{"score", "age"})
	if !reflect.DeepEqual(asSet(fields), wantSet) {
		t.Fatalf("indexed fields list changed unexpectedly, got=%v", fields)
	}
}

func asSet(xs []string) map[string]struct{} {
	m := make(map[string]struct{}, len(xs))
	for _, x := range xs {
		m[x] = struct{}{}
	}
	return m
}
