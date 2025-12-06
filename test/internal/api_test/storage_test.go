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
	storage.LoadJsonDB()
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

	all := api_storage.ListEntities(entity, 0, 0, "", true, nil, "", "")
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

	if list := api_storage.ListEntities("posts", 0, 0, "", true, nil, "", ""); len(list) != 0 {
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

func TestListEntities_WithFilters(t *testing.T) {
	initTestStore(t)

	entity := "books"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "b1", "title": "Go in Action", "author": "Alice"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "b2", "title": "Learning Python", "author": "Bob"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "b3", "title": "Advanced Go", "author": "Alice"})

	filters := map[string]map[string]string{"author": {"eq": "Alice"}}
	results := api_storage.ListEntities(entity, 0, 0, "", true, filters, "", "")
	if len(results) != 2 {
		t.Fatalf("expected 2 results for author=Alice, got %d (%v)", len(results), results)
	}

	filters = map[string]map[string]string{"title": {"eq": "Learning Python"}}
	results = api_storage.ListEntities(entity, 0, 0, "", true, filters, "", "")
	if len(results) != 1 || results[0]["id"] != "b2" {
		t.Fatalf("expected only b2, got %v", results)
	}

	filters = map[string]map[string]string{"author": {"eq": "Al*"}}
	results = api_storage.ListEntities(entity, 0, 0, "", true, filters, "", "")
	if len(results) != 2 {
		t.Fatalf("expected 2 results for author wildcard, got %d (%v)", len(results), results)
	}

	filters = map[string]map[string]string{"title": {"eq": "*Go*"}}
	results = api_storage.ListEntities(entity, 0, 0, "", true, filters, "", "")
	if len(results) != 2 {
		t.Fatalf("expected 2 results for title contains Go, got %d (%v)", len(results), results)
	}

	filters = map[string]map[string]string{"author": {"neq": "Alice"}}
	results = api_storage.ListEntities(entity, 0, 0, "", true, filters, "", "")
	if len(results) != 1 || results[0]["id"] != "b2" {
		t.Fatalf("expected only b2 with neq filter, got %v", results)
	}
}

func TestListEntities_WithNestedFilters(t *testing.T) {
	initTestStore(t)

	entity := "articles"
	api_storage.WriteEntity(entity, map[string]interface{}{
		"id":    "a1",
		"title": "Go & Dist",
		"author": map[string]interface{}{
			"name": "Mister Good",
			"id":   "u1",
			"category": map[string]interface{}{
				"title": "yep",
			},
		},
	})
	api_storage.WriteEntity(entity, map[string]interface{}{
		"id":    "a2",
		"title": "Python Tips",
		"author": map[string]interface{}{
			"name": "Alice",
			"id":   "u2",
			"category": map[string]interface{}{
				"title": "news",
			},
		},
	})
	api_storage.WriteEntity(entity, map[string]interface{}{
		"id":    "a3",
		"title": "Go Advanced",
		"author": map[string]interface{}{
			"name": "Bob",
			"id":   "u3",
			"category": map[string]interface{}{
				"title": "yep",
			},
		},
	})

	f1 := map[string]map[string]string{"author.name": {"eq": "Alice"}}
	r1 := api_storage.ListEntities(entity, 0, 0, "", true, f1, "", "")
	if len(r1) != 1 || r1[0]["id"] != "a2" {
		t.Fatalf("expected only a2, got %v", r1)
	}

	f2 := map[string]map[string]string{"author.name": {"eq": "mister*"}}
	r2 := api_storage.ListEntities(entity, 0, 0, "", true, f2, "", "")
	if len(r2) != 1 || r2[0]["id"] != "a1" {
		t.Fatalf("expected only a1, got %v", r2)
	}

	f3 := map[string]map[string]string{"author.category.title": {"eq": "yep"}}
	r3 := api_storage.ListEntities(entity, 0, 0, "", true, f3, "", "")
	if len(r3) != 2 {
		t.Fatalf("expected a1 and a3 for category=yep, got %v", r3)
	}

	f4 := map[string]map[string]string{
		"author.category.title": {"eq": "yep"},
		"author.name":           {"neq": "bob"},
	}
	r4 := api_storage.ListEntities(entity, 0, 0, "", true, f4, "", "")
	if len(r4) != 1 || r4[0]["id"] != "a1" {
		t.Fatalf("expected only a1 for combined eq/neq, got %v", r4)
	}
}

func TestListEntities_WithNumericFilters(t *testing.T) {
	initTestStore(t)

	entity := "products"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "p1", "price": float64(10)})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "p2", "price": float64(20)})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "p3", "price": float64(30)})

	f1 := map[string]map[string]string{"price": {"eq": "20"}}
	r1 := api_storage.ListEntities(entity, 0, 0, "", true, f1, "", "")
	if len(r1) != 1 || r1[0]["id"] != "p2" {
		t.Fatalf("expected only p2 for eq=20, got %v", r1)
	}

	f2 := map[string]map[string]string{"price": {"neq": "20"}}
	r2 := api_storage.ListEntities(entity, 0, 0, "", true, f2, "", "")
	if len(r2) != 2 {
		t.Fatalf("expected 2 results for neq=20, got %v", r2)
	}

	f3 := map[string]map[string]string{"price": {"lt": "20"}}
	r3 := api_storage.ListEntities(entity, 0, 0, "", true, f3, "", "")
	if len(r3) != 1 || r3[0]["id"] != "p1" {
		t.Fatalf("expected only p1 for lt=20, got %v", r3)
	}

	f4 := map[string]map[string]string{"price": {"lte": "20"}}
	r4 := api_storage.ListEntities(entity, 0, 0, "", true, f4, "", "")
	if len(r4) != 2 {
		t.Fatalf("expected p1 and p2 for lte=20, got %v", r4)
	}

	f5 := map[string]map[string]string{"price": {"gt": "20"}}
	r5 := api_storage.ListEntities(entity, 0, 0, "", true, f5, "", "")
	if len(r5) != 1 || r5[0]["id"] != "p3" {
		t.Fatalf("expected only p3 for gt=20, got %v", r5)
	}

	f6 := map[string]map[string]string{"price": {"gte": "20"}}
	r6 := api_storage.ListEntities(entity, 0, 0, "", true, f6, "", "")
	if len(r6) != 2 {
		t.Fatalf("expected p2 and p3 for gte=20, got %v", r6)
	}
}

func TestOffsetLimitAndSort(t *testing.T) {
	initTestStore(t)

	entity := "nums"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "1", "v": 1})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "2", "v": 2})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "3", "v": 3})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "4", "v": 4})

	all := api_storage.ListEntities(entity, 2, 1, "", true, nil, "", "")
	if len(all) != 2 {
		t.Fatalf("expected 2 after offset/limit, got %v", all)
	}
}

func TestCountEntities_OK(t *testing.T) {
	initTestStore(t)

	entity := "cars"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c1", "brand": "bmw"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c2", "brand": "audi"})

	n, err := api_storage.CountEntities(entity)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if n != 2 {
		t.Fatalf("expected 2, got %d", n)
	}
}

func TestCountEntities_EmptyEntity(t *testing.T) {
	initTestStore(t)

	entity := "empty"
	_, err := api_storage.CountEntities(entity)
	if err == nil {
		t.Fatal("expected error for empty entity, got nil")
	}
}

func TestCountEntities_EntityDoesNotExist(t *testing.T) {
	initTestStore(t)

	_, err := api_storage.CountEntities("ghost")
	if err == nil {
		t.Fatal("expected error for missing entity, got nil")
	}
}

func TestCountEntities_IgnoresOtherEntities(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("movies", map[string]interface{}{"id": "m1"})
	api_storage.WriteEntity("movies", map[string]interface{}{"id": "m2"})
	api_storage.WriteEntity("songs", map[string]interface{}{"id": "s1"})

	n, err := api_storage.CountEntities("movies")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if n != 2 {
		t.Fatalf("expected 2, got %d", n)
	}
}

func TestEntityExists_True(t *testing.T) {
	initTestStore(t)

	entity := "cars"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c1", "brand": "bmw"})

	if !api_storage.EntityExists(entity, "c1") {
		t.Fatalf("expected EntityExists=true for c1")
	}
}

func TestEntityExists_False(t *testing.T) {
	initTestStore(t)

	entity := "cars"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c1", "brand": "bmw"})

	if api_storage.EntityExists(entity, "doesnotexist") {
		t.Fatalf("expected EntityExists=false for missing id")
	}
}

func TestEntityExists_WrongEntity(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("authors", map[string]interface{}{"id": "a1", "name": "Alice"})

	if api_storage.EntityExists("books", "a1") {
		t.Fatalf("expected EntityExists=false for mismatched entity")
	}
}

func TestEntityExists_EmptyStore(t *testing.T) {
	initTestStore(t)

	if api_storage.EntityExists("ghost", "id123") {
		t.Fatalf("expected EntityExists=false for empty store")
	}
}

func TestEntityTypesAndExists(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("alpha", map[string]interface{}{"id": "a1"})
	api_storage.WriteEntity("beta", map[string]interface{}{"id": "b1"})
	types := api_storage.ListEntityTypes()
	set := map[string]bool{}
	for _, t0 := range types {
		set[t0] = true
	}
	if !set["alpha"] || !set["beta"] {
		t.Fatalf("expected alpha and beta in types, got %v", types)
	}
	if !api_storage.EntityTypeExists("alpha") || api_storage.EntityTypeExists("gamma") {
		t.Fatalf("EntityTypeExists mismatch")
	}
}

func TestWriteListAndUpdateList(t *testing.T) {
	initTestStore(t)

	api_storage.WriteListOfEntities("list", []map[string]interface{}{
		{"id": "1", "v": float64(1)},
		{"id": "2", "v": float64(2)},
	})

	updated := api_storage.UpdateListOfEntities("list", []map[string]interface{}{
		{"id": "1", "v": float64(9)},
		{"id": "2", "v": float64(8)},
		{"v": float64(7)},
	})

	if len(updated) != 2 {
		t.Fatalf("expected 2 updated, got %v", updated)
	}

	v1 := api_storage.ReadEntityById("list", "1")["v"]
	if v1 != float64(9) {
		t.Fatalf("update list failed, got %v", v1)
	}

	v2 := api_storage.ReadEntityById("list", "2")["v"]
	if v2 != float64(8) {
		t.Fatalf("update list failed, got %v", v2)
	}

	missing := api_storage.ReadEntityById("list", "")
	if missing != nil {
		t.Fatalf("entity without id should not exist")
	}
}

func TestDeleteAllAndCountAll(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("a", map[string]interface{}{"id": "1"})
	api_storage.WriteEntity("b", map[string]interface{}{"id": "1"})
	api_storage.WriteEntity("b", map[string]interface{}{"id": "2"})

	if api_storage.CountAllEntities() != 3 {
		t.Fatalf("expected 3 before deleteall")
	}

	api_storage.DeleteAll()

	if api_storage.CountAllEntities() != 0 {
		t.Fatalf("expected 0 after deleteall")
	}
}

func TestDumpAllAndImportAll(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("x", map[string]interface{}{"id": "1", "v": 1})
	api_storage.WriteEntity("y", map[string]interface{}{"id": "2", "v": 2})

	d := api_storage.DumpAll()
	if len(d) != 2 {
		t.Fatalf("dump size mismatch: %v", d)
	}

	initTestStore(t)

	api_storage.ImportAll(map[string][]map[string]interface{}{
		"x": {{"id": "1", "v": 1}, {"id": "3", "v": 3}},
	})
	if api_storage.CountAllEntities() != 2 {
		t.Fatalf("import count mismatch")
	}
	if api_storage.ReadEntityById("x", "3") == nil {
		t.Fatalf("imported entity missing")
	}
}

func TestListEntities_AutoIncludeNested(t *testing.T) {
	initTestStore(t)

	entity := "articles"
	api_storage.WriteEntity(entity, map[string]interface{}{
		"id":    "a1",
		"title": "X",
		"author": map[string]interface{}{
			"id":   "u1",
			"name": "Alice",
		},
	})
	api_storage.WriteEntity(entity, map[string]interface{}{
		"id":    "a2",
		"title": "Y",
		"author": map[string]interface{}{
			"id":   "u2",
			"name": "Bob",
		},
	})

	filters := map[string]map[string]string{"author.name": {"eq": "Alice"}}
	res := api_storage.ListEntities(entity, 0, 0, "", true, filters, "", "")

	if len(res) != 1 || res[0]["id"] != "a1" {
		t.Fatalf("expected auto-include match a1, got %v", res)
	}

	_, ok := res[0]["author"].(map[string]interface{})
	if !ok {
		t.Fatalf("expected author sub-entity to be expanded automatically, got %v", res[0])
	}
}

func TestListEntities_AutoIncludeDeepNested(t *testing.T) {
	initTestStore(t)

	entity := "articles"
	api_storage.WriteEntity(entity, map[string]interface{}{
		"id":    "a1",
		"title": "X",
		"author": map[string]interface{}{
			"id":   "u1",
			"name": "Alice",
			"profile": map[string]interface{}{
				"city": "Paris",
			},
		},
	})
	api_storage.WriteEntity(entity, map[string]interface{}{
		"id":    "a2",
		"title": "Y",
		"author": map[string]interface{}{
			"id":   "u2",
			"name": "Bob",
			"profile": map[string]interface{}{
				"city": "Lyon",
			},
		},
	})

	filters := map[string]map[string]string{"author.profile.city": {"eq": "Paris"}}
	res := api_storage.ListEntities(entity, 0, 0, "", true, filters, "", "")

	if len(res) != 1 || res[0]["id"] != "a1" {
		t.Fatalf("expected only a1, got %v", res)
	}

	author := res[0]["author"].(map[string]interface{})
	profile := author["profile"].(map[string]interface{})
	if profile["city"] != "Paris" {
		t.Fatalf("expected nested include applied, got %v", res[0])
	}
}
