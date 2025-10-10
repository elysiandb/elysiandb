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

	if !reflect.DeepEqual(asc, []string{"b", "a", "c"}) {
		t.Fatalf("asc=%v", asc)
	}
	if !reflect.DeepEqual(desc, []string{"c", "a", "b"}) {
		t.Fatalf("desc=%v", desc)
	}

	fields := api_storage.GetListForIndexedFields(entity)
	if !reflect.DeepEqual(fields, []string{"score"}) {
		t.Fatalf("fields=%v", fields)
	}
}

func TestEnsureFieldIndex_Nested(t *testing.T) {
	initIdxTestStore(t)

	entity := "idx_nested"
	api_storage.WriteEntity(entity, map[string]any{
		"id": "n1",
		"obj": map[string]any{
			"sub": map[string]any{
				"val": 42,
			},
		},
	})

	if !api_storage.IndexExistsForField(entity, "obj.sub.val") {
		t.Fatalf("expected nested index obj.sub.val to exist")
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
	if !reflect.DeepEqual(asc, []string{"p2", "p1"}) {
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

	if _, dirty := api_storage.DirtyFields.Load(entity + "|score"); !dirty {
		t.Fatalf("expected 'score' index to be marked dirty or removed")
	}
	if !api_storage.IndexExistsForField(entity, "age") {
		t.Fatalf("index for 'age' should still exist")
	}
}

func asSet(xs []string) map[string]struct{} {
	m := make(map[string]struct{}, len(xs))
	for _, x := range xs {
		m[x] = struct{}{}
	}
	return m
}

func TestUpdateIndexesForEntity_SimpleChange(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_update_simple"
	oldData := map[string]interface{}{"id": "1", "name": "Alice", "age": 30}
	newData := map[string]interface{}{"id": "1", "name": "Bob", "age": 30}
	api_storage.UpdateIndexesForEntity(entity, "1", oldData, newData)
	if !api_storage.IndexExistsForField(entity, "name") {
		t.Fatalf("expected index for 'name'")
	}
}

func TestUpdateIndexesForEntity_AddField(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_update_add"
	oldData := map[string]interface{}{"id": "1", "name": "Alice"}
	newData := map[string]interface{}{"id": "1", "name": "Alice", "city": "Paris"}
	api_storage.UpdateIndexesForEntity(entity, "1", oldData, newData)
	if !api_storage.IndexExistsForField(entity, "city") {
		t.Fatalf("expected index for 'city'")
	}
}

func TestUpdateIndexesForEntity_RemoveField(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_update_remove"
	oldData := map[string]interface{}{"id": "1", "country": "FR", "age": 40}
	newData := map[string]interface{}{"id": "1", "age": 40}
	api_storage.UpdateIndexesForEntity(entity, "1", oldData, newData)

	if _, dirty := api_storage.DirtyFields.Load(entity + "|country"); !dirty {
		t.Fatalf("expected index for 'country' to be marked dirty or removed")
	}
}

func TestUpdateIndexesForEntity_ArrayChange(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_update_array"
	oldData := map[string]interface{}{"id": "1", "tags": []interface{}{"a", "b"}}
	newData := map[string]interface{}{"id": "1", "tags": []interface{}{"a", "b", "c"}}
	api_storage.UpdateIndexesForEntity(entity, "1", oldData, newData)
	if !api_storage.IndexExistsForField(entity, "tags") {
		t.Fatalf("expected index for 'tags'")
	}
}

func TestUpdateIndexesForEntity_MapChange(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_update_map"
	oldData := map[string]interface{}{"id": "1", "info": map[string]interface{}{"a": 1, "b": 2}}
	newData := map[string]interface{}{"id": "1", "info": map[string]interface{}{"a": 1, "b": 3}}
	api_storage.UpdateIndexesForEntity(entity, "1", oldData, newData)
	if !api_storage.IndexExistsForField(entity, "info") {
		t.Fatalf("expected index for 'info'")
	}
}

func TestUpdateIndexesForEntity_NoChange(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_update_nochange"
	oldData := map[string]interface{}{"id": "1", "x": 10}
	newData := map[string]interface{}{"id": "1", "x": 10}
	api_storage.UpdateIndexesForEntity(entity, "1", oldData, newData)
	if !api_storage.IndexExistsForField(entity, "x") {
		t.Fatalf("expected index for 'x'")
	}
}

func TestUpdateIndexesForEntity_ComplexNestedChange(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_update_nested"
	oldData := map[string]interface{}{
		"id": "1",
		"obj": map[string]interface{}{
			"a": []interface{}{"x", "y"},
			"b": 10,
		},
	}
	newData := map[string]interface{}{
		"id": "1",
		"obj": map[string]interface{}{
			"a": []interface{}{"x", "z"},
			"b": 11,
		},
	}
	api_storage.UpdateIndexesForEntity(entity, "1", oldData, newData)
	if !api_storage.IndexExistsForField(entity, "obj.a") {
		t.Fatalf("expected index for 'obj.a'")
	}
	if !api_storage.IndexExistsForField(entity, "obj.b") {
		t.Fatalf("expected index for 'obj.b'")
	}
}

func TestUpdateIndexesForEntity_MultipleChanges(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_update_multi"
	oldData := map[string]interface{}{"id": "1", "name": "A", "age": 20, "tags": []interface{}{"t1"}}
	newData := map[string]interface{}{"id": "1", "name": "B", "age": 25, "tags": []interface{}{"t2", "t3"}}
	api_storage.UpdateIndexesForEntity(entity, "1", oldData, newData)
	for _, f := range []string{"name", "age", "tags"} {
		if !api_storage.IndexExistsForField(entity, f) {
			t.Fatalf("expected index for '%s'", f)
		}
	}
}

func TestMarkFieldDirtyAndEnsureFresh(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_lazy_refresh"
	field := "price"
	api_storage.MarkFieldDirty(entity, field)

	if _, dirty := api_storage.DirtyFields.Load(entity + "|" + field); !dirty {
		t.Fatalf("expected field to be marked dirty")
	}

	api_storage.EnsureFieldIndex(entity, field, "1", 100)
	_ = api_storage.IndexExistsForField(entity, field)
	if _, dirty := api_storage.DirtyFields.Load(entity + "|" + field); dirty {
		t.Fatalf("expected dirty flag cleared after ensureFieldIndex")
	}
}

func TestProcessNextDirtyField(t *testing.T) {
	initIdxTestStore(t)
	entity := "idx_process_dirty"
	api_storage.WriteEntity(entity, map[string]any{"id": "x1", "qty": 10})
	api_storage.MarkFieldDirty(entity, "qty")

	api_storage.ProcessNextDirtyField()

	api_storage.ProcessNextDirtyField()
	_ = api_storage.IndexExistsForField(entity, "qty")
	if _, dirty := api_storage.DirtyFields.Load(entity + "|" + "qty"); dirty {
		t.Fatalf("expected dirty flag cleared after processing")
	}
	
	if !api_storage.IndexExistsForField(entity, "qty") {
		t.Fatalf("expected index rebuilt for qty")
	}
}
