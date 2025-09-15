package api_test

import (
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func initTestStore(t *testing.T) {
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
}

func TestWriteAndReadEntityById(t *testing.T) {
	initTestStore(t)

	entity := "articles"
	id := "a1"
	in := map[string]interface{}{
		"id":    id,
		"title": "Hello",
		"tags":  []interface{}{"go", "kv"},
	}

	api_storage.WriteEntity(entity, in)

	got := api_storage.ReadEntityById(entity, id)
	if got == nil {
		t.Fatalf("ReadEntityById returned nil")
	}
	if got["id"] != id {
		t.Fatalf("id mismatch: got %v want %v", got["id"], id)
	}
	if got["title"] != "Hello" {
		t.Fatalf("title mismatch: got %v want %v", got["title"], "Hello")
	}
	if tags, ok := got["tags"].([]interface{}); !ok || len(tags) != 2 || tags[0] != "go" || tags[1] != "kv" {
		t.Fatalf("tags mismatch: got %#v", got["tags"])
	}
}

func TestReadAllEntities(t *testing.T) {
	initTestStore(t)

	entity := "users"
	u1 := map[string]interface{}{"id": "u1", "name": "alice"}
	u2 := map[string]interface{}{"id": "u2", "name": "bob"}
	api_storage.WriteEntity(entity, u1)
	api_storage.WriteEntity(entity, u2)

	all := api_storage.ListEntities(entity, 0, 0, "", true)
	if len(all) != 2 {
		t.Fatalf("ListEntities len=%d, want 2, all=%v", len(all), all)
	}

	seen := map[string]bool{}
	for _, it := range all {
		seen[it["id"].(string)] = true
	}
	if !seen["u1"] || !seen["u2"] {
		t.Fatalf("expected to see u1 and u2, got %v", seen)
	}
}

func TestUpdateEntityById_MergesAndPersists(t *testing.T) {
	initTestStore(t)

	entity := "orders"
	id := "o42"
	api_storage.WriteEntity(entity, map[string]interface{}{
		"id":     id,
		"status": "pending",
		"price":  10,
	})

	updated := api_storage.UpdateEntityById(entity, id, map[string]interface{}{
		"status": "paid",
		"note":   "ok",
	})
	if updated == nil {
		t.Fatalf("UpdateEntityById returned nil")
	}
	if updated["status"] != "paid" {
		t.Fatalf("status not updated: %v", updated["status"])
	}
	if updated["note"] != "ok" {
		t.Fatalf("note not merged: %v", updated["note"])
	}
	got := api_storage.ReadEntityById(entity, id)
	if !reflect.DeepEqual(normalizeMap(updated), normalizeMap(got)) {
		t.Fatalf("persisted mismatch:\n updated=%v\n got=%v", updated, got)
	}
}

func TestDeleteEntityById_RemovesSingle(t *testing.T) {
	initTestStore(t)

	entity := "comments"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c1", "body": "hello"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c2", "body": "world"})

	api_storage.DeleteEntityById(entity, "c1")

	if v := api_storage.ReadEntityById(entity, "c1"); v != nil {
		t.Fatalf("c1 should be deleted, got %v", v)
	}
	if v := api_storage.ReadEntityById(entity, "c2"); v == nil {
		t.Fatalf("c2 should still exist")
	}
}

func TestDeleteAllEntities_RemovesAllForThatEntityOnly(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("posts", map[string]interface{}{"id": "p1"})
	api_storage.WriteEntity("posts", map[string]interface{}{"id": "p2"})
	api_storage.WriteEntity("profiles", map[string]interface{}{"id": "me"})

	api_storage.DeleteAllEntities("posts")

	if list := api_storage.ListEntities("posts", 0, 0, "", true); len(list) != 0 {
		t.Fatalf("posts should be empty, got %v", list)
	}
	if v := api_storage.ReadEntityById("profiles", "me"); v == nil {
		t.Fatalf("profiles entity should be untouched")
	}
}

func normalizeMap(m map[string]interface{}) map[string]interface{} {
	out := make(map[string]interface{}, len(m))
	for k, v := range m {
		switch tv := v.(type) {
		case []interface{}:
			cp := make([]interface{}, len(tv))
			copy(cp, tv)
			out[k] = cp
		case map[string]interface{}:
			out[k] = normalizeMap(tv)
		default:
			out[k] = v
		}
	}
	return out
}
