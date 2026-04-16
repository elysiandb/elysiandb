package api_test

import (
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
)

func seedParityData(t *testing.T) {
	t.Helper()
	initTestStore(t)

	api_storage.WriteEntity("user", map[string]any{
		"id": "u1", "name": "Alice", "age": float64(30),
		"tags": []any{"go", "rust"},
		"address": map[string]any{"city": "Paris", "country": "France"},
	})
	api_storage.WriteEntity("user", map[string]any{
		"id": "u2", "name": "Bob", "age": float64(25),
		"tags": []any{"python", "go"},
		"address": map[string]any{"city": "London", "country": "UK"},
	})
	api_storage.WriteEntity("user", map[string]any{
		"id": "u3", "name": "Charlie", "age": float64(35),
		"tags": []any{"rust", "c++"},
		"address": map[string]any{"city": "Berlin", "country": "Germany"},
	})

	api_storage.WriteEntity("post", map[string]any{
		"id": "p1", "title": "Go patterns", "body": "Content about Go",
		"author": map[string]any{"@entity": "user", "id": "u1"},
	})
	api_storage.WriteEntity("post", map[string]any{
		"id": "p2", "title": "Rust tricks", "body": "Content about Rust",
		"author": map[string]any{"@entity": "user", "id": "u3"},
	})

	api_storage.WriteEntity("profile", map[string]any{
		"id": "pr1", "bio": "Developer", "website": "https://alice.dev",
	})
}

func TestParity_CRUD(t *testing.T) {
	seedParityData(t)

	u := api_storage.ReadEntityById("user", "u1")
	if u == nil || u["name"] != "Alice" {
		t.Fatal("read failed")
	}

	api_storage.UpdateEntityById("user", "u1", map[string]any{"name": "Alice Updated"})
	u = api_storage.ReadEntityById("user", "u1")
	if u["name"] != "Alice Updated" {
		t.Fatal("update failed")
	}

	api_storage.DeleteEntityById("user", "u3")
	if api_storage.ReadEntityById("user", "u3") != nil {
		t.Fatal("delete failed")
	}

	all := api_storage.ListEntities("user", 0, 0, "", true, nil, "", "")
	if len(all) != 2 {
		t.Fatalf("expected 2 users after delete, got %d", len(all))
	}
}

func TestParity_ListWithFilters_Eq(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"name": {"eq": "Alice"}}, "", "")
	if len(result) != 1 || result[0]["id"] != "u1" {
		t.Fatalf("eq filter: expected [u1], got %v", result)
	}
}

func TestParity_ListWithFilters_GlobEq(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"name": {"eq": "*li*"}}, "", "")
	if len(result) != 2 {
		t.Fatalf("glob eq: expected 2 (Alice, Charlie), got %d", len(result))
	}
}

func TestParity_ListWithFilters_Neq(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"name": {"neq": "Alice"}}, "", "")
	if len(result) != 2 {
		t.Fatalf("neq filter: expected 2, got %d", len(result))
	}
}

func TestParity_ListWithFilters_Gte(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"age": {"gte": "30"}}, "", "")
	if len(result) != 2 {
		t.Fatalf("gte filter: expected 2 (Alice, Charlie), got %d", len(result))
	}
}

func TestParity_ListWithFilters_Lt(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"age": {"lt": "30"}}, "", "")
	if len(result) != 1 || result[0]["id"] != "u2" {
		t.Fatalf("lt filter: expected [u2], got %v", result)
	}
}

func TestParity_ListWithFilters_Contains(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"tags": {"contains": "go"}}, "", "")
	if len(result) != 2 {
		t.Fatalf("contains filter: expected 2 (Alice, Bob), got %d", len(result))
	}
}

func TestParity_ListWithFilters_NotContains(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"tags": {"not_contains": "go"}}, "", "")
	if len(result) != 1 || result[0]["id"] != "u3" {
		t.Fatalf("not_contains filter: expected [u3], got %v", result)
	}
}

func TestParity_ListWithFilters_Any(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"tags": {"any": "python,c++"}}, "", "")
	if len(result) != 2 {
		t.Fatalf("any filter: expected 2 (Bob, Charlie), got %d", len(result))
	}
}

func TestParity_ListWithFilters_All(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"tags": {"all": "go,rust"}}, "", "")
	if len(result) != 1 || result[0]["id"] != "u1" {
		t.Fatalf("all filter: expected [u1], got %v", result)
	}
}

func TestParity_ListWithFilters_None(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true,
		map[string]map[string]string{"tags": {"none": "go"}}, "", "")
	if len(result) != 1 || result[0]["id"] != "u3" {
		t.Fatalf("none filter: expected [u3], got %v", result)
	}
}

func TestParity_Search(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true, nil, "*Paris*", "")
	if len(result) != 1 || result[0]["id"] != "u1" {
		t.Fatalf("search: expected [u1], got %v", result)
	}
}

func TestParity_SearchArray(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("user", 0, 0, "", true, nil, "*rust*", "")
	if len(result) != 2 {
		t.Fatalf("search array: expected 2 (Alice, Charlie), got %d", len(result))
	}
}

func TestParity_Pagination(t *testing.T) {
	seedParityData(t)

	page1 := api_storage.ListEntities("user", 2, 0, "", true, nil, "", "")
	if len(page1) != 2 {
		t.Fatalf("page 1: expected 2, got %d", len(page1))
	}

	page2 := api_storage.ListEntities("user", 2, 2, "", true, nil, "", "")
	if len(page2) != 1 {
		t.Fatalf("page 2: expected 1, got %d", len(page2))
	}

	if page1[0]["id"] == page2[0]["id"] {
		t.Fatal("pagination: page1 and page2 should not overlap")
	}
}

func TestParity_Includes_Simple(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("post", 0, 0, "", true, nil, "", "author")
	if len(result) != 2 {
		t.Fatalf("includes: expected 2, got %d", len(result))
	}

	for _, post := range result {
		author, ok := post["author"].(map[string]any)
		if !ok {
			t.Fatalf("author should be resolved, got %T", post["author"])
		}

		if _, ok := author["name"]; !ok {
			t.Fatal("author should have name field after include resolution")
		}
	}
}

func TestParity_Includes_All(t *testing.T) {
	seedParityData(t)

	result := api_storage.ListEntities("post", 0, 0, "", true, nil, "", "all")
	if len(result) != 2 {
		t.Fatalf("includes all: expected 2, got %d", len(result))
	}

	for _, post := range result {
		author, ok := post["author"].(map[string]any)
		if !ok {
			t.Fatalf("author should be resolved with 'all', got %T", post["author"])
		}

		if _, ok := author["name"]; !ok {
			t.Fatal("author should have name field")
		}
	}
}

func TestParity_Includes_Nested(t *testing.T) {
	seedParityData(t)

	api_storage.UpdateEntityById("user", "u1", map[string]any{
		"profile": map[string]any{"@entity": "profile", "id": "pr1"},
	})

	result := api_storage.ListEntities("post", 0, 0, "", true, nil, "", "author.profile")
	found := false
	for _, post := range result {
		if post["id"] == "p1" {
			found = true
			author, ok := post["author"].(map[string]any)
			if !ok {
				t.Fatalf("author should be resolved, got %T", post["author"])
			}

			profile, ok := author["profile"].(map[string]any)
			if !ok {
				t.Fatalf("profile should be resolved, got %T", author["profile"])
			}

			if profile["bio"] != "Developer" {
				t.Fatalf("expected bio=Developer, got %v", profile["bio"])
			}
		}
	}

	if !found {
		t.Fatal("post p1 not found")
	}
}

func TestParity_FilterFields_DotNotation(t *testing.T) {
	seedParityData(t)

	u := api_storage.ReadEntityById("user", "u1")
	filtered := api_storage.FilterFields(u, []string{"name", "address.city"})

	if filtered["name"] != "Alice" {
		t.Fatalf("expected name=Alice, got %v", filtered["name"])
	}

	addr, ok := filtered["address"].(map[string]any)
	if !ok {
		t.Fatalf("expected nested address, got %T", filtered["address"])
	}

	if addr["city"] != "Paris" {
		t.Fatalf("expected city=Paris, got %v", addr["city"])
	}

	if _, ok := addr["country"]; ok {
		t.Fatal("country should be filtered out")
	}
}

func TestParity_ApplyFiltersToList(t *testing.T) {
	entities := []map[string]any{
		{"id": "1", "name": "Alice", "age": float64(30)},
		{"id": "2", "name": "Bob", "age": float64(25)},
		{"id": "3", "name": "Charlie", "age": float64(35)},
	}

	result := api_storage.ApplyFiltersToList(entities, map[string]map[string]string{
		"age": {"gt": "25"},
	})

	if len(result) != 2 {
		t.Fatalf("expected 2, got %d", len(result))
	}
}

func TestParity_GetListOfIds(t *testing.T) {
	seedParityData(t)

	ids, err := api_storage.GetListOfIds("user", "", true)
	if err != nil {
		t.Fatal(err)
	}

	if len(ids) != 3 {
		t.Fatalf("expected 3 ids, got %d", len(ids))
	}

	for _, id := range ids {
		if id != "u1" && id != "u2" && id != "u3" {
			t.Fatalf("unexpected id: %s", id)
		}
	}
}

func TestParity_ImportExport(t *testing.T) {
	seedParityData(t)

	dump := api_storage.DumpAll()
	if dump == nil {
		t.Fatal("DumpAll returned nil")
	}

	if _, ok := dump["user"]; !ok {
		t.Fatal("expected user entity type in dump")
	}

	if _, ok := dump["post"]; !ok {
		t.Fatal("expected post entity type in dump")
	}
}

func TestParity_EntityTypeLifecycle(t *testing.T) {
	initTestStore(t)

	if api_storage.EntityTypeExists("widget") {
		t.Fatal("widget should not exist yet")
	}

	api_storage.CreateEntityType("widget")
	if !api_storage.EntityTypeExists("widget") {
		t.Fatal("widget should exist after creation")
	}

	types := api_storage.ListEntityTypes()
	found := false
	for _, ty := range types {
		if ty == "widget" {
			found = true
		}
	}

	if !found {
		t.Fatal("widget not in ListEntityTypes")
	}

	api_storage.DeleteEntityType("widget")
	if api_storage.EntityTypeExists("widget") {
		t.Fatal("widget should not exist after deletion")
	}
}

func TestParity_DeleteAllEntities_KeepsType(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("item", map[string]any{"id": "i1", "name": "a"})
	api_storage.WriteEntity("item", map[string]any{"id": "i2", "name": "b"})

	if !api_storage.EntityTypeExists("item") {
		t.Fatal("item type should exist")
	}

	all := api_storage.ListEntities("item", 0, 0, "", true, nil, "", "")
	if len(all) != 2 {
		t.Fatalf("expected 2 items, got %d", len(all))
	}

	api_storage.DeleteAllEntities("item")

	all = api_storage.ListEntities("item", 0, 0, "", true, nil, "", "")
	if len(all) != 0 {
		t.Fatalf("expected 0 items after DeleteAllEntities, got %d", len(all))
	}
}

func TestParity_CountAllEntities(t *testing.T) {
	seedParityData(t)

	count := api_storage.CountAllEntities()
	if count < 5 {
		t.Fatalf("expected at least 5 entities, got %d", count)
	}
}
