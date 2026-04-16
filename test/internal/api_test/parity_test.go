package api_test

import (
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
)

func TestApplyFiltersToList(t *testing.T) {
	initTestStore(t)

	entities := []map[string]any{
		{"id": "1", "name": "alice", "age": float64(25)},
		{"id": "2", "name": "bob", "age": float64(30)},
		{"id": "3", "name": "charlie", "age": float64(35)},
	}

	filters := map[string]map[string]string{
		"age": {"gte": "30"},
	}

	result := api_storage.ApplyFiltersToList(entities, filters)
	if len(result) != 2 {
		t.Fatalf("expected 2 results, got %d", len(result))
	}

	ids := map[string]bool{}
	for _, e := range result {
		ids[e["id"].(string)] = true
	}

	if !ids["2"] || !ids["3"] {
		t.Fatalf("expected bob and charlie, got %v", ids)
	}
}

func TestApplyFiltersToListEmpty(t *testing.T) {
	entities := []map[string]any{
		{"id": "1", "name": "alice"},
	}

	result := api_storage.ApplyFiltersToList(entities, nil)
	if len(result) != 1 {
		t.Fatal("empty filters should return all entities")
	}
}

func TestSearchMatchesEntity(t *testing.T) {
	entity := map[string]any{
		"id":   "1",
		"name": "alice",
		"tags": []any{"go", "rust"},
		"meta": map[string]any{
			"city": "Paris",
		},
	}

	if !api_storage.SearchMatchesEntity(entity, "*alice*") {
		t.Fatal("should match name")
	}

	if !api_storage.SearchMatchesEntity(entity, "*go*") {
		t.Fatal("should match array element")
	}

	if !api_storage.SearchMatchesEntity(entity, "*Paris*") {
		t.Fatal("should match nested field")
	}

	if api_storage.SearchMatchesEntity(entity, "*nonexistent*") {
		t.Fatal("should not match")
	}
}

func TestFilterFieldsDotNotation(t *testing.T) {
	data := map[string]any{
		"id":   "1",
		"name": "John",
		"address": map[string]any{
			"city":    "Paris",
			"country": "France",
		},
	}

	result := api_storage.FilterFields(data, []string{"name", "address.city"})

	if result["name"] != "John" {
		t.Fatalf("expected name=John, got %v", result["name"])
	}

	addr, ok := result["address"].(map[string]any)
	if !ok {
		t.Fatalf("expected address to be map, got %T", result["address"])
	}

	if addr["city"] != "Paris" {
		t.Fatalf("expected city=Paris, got %v", addr["city"])
	}

	if _, ok := addr["country"]; ok {
		t.Fatal("expected country to be filtered out")
	}
}

func TestFiltersMatchEntityContains(t *testing.T) {
	entity := map[string]any{
		"id":   "1",
		"tags": []any{"go", "rust", "python"},
	}

	filters := map[string]map[string]string{
		"tags": {"contains": "go"},
	}

	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatal("should match: tags contains go")
	}

	filters = map[string]map[string]string{
		"tags": {"contains": "java"},
	}

	if api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatal("should not match: tags does not contain java")
	}
}

func TestFiltersMatchEntityNotContains(t *testing.T) {
	entity := map[string]any{
		"id":   "1",
		"tags": []any{"go", "rust"},
	}

	filters := map[string]map[string]string{
		"tags": {"not_contains": "java"},
	}

	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatal("should match: tags does not contain java")
	}

	filters = map[string]map[string]string{
		"tags": {"not_contains": "go"},
	}

	if api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatal("should not match: tags contains go")
	}
}

func TestFiltersMatchEntityNestedField(t *testing.T) {
	entity := map[string]any{
		"id": "1",
		"address": map[string]any{
			"city":    "Paris",
			"country": "France",
		},
	}

	filters := map[string]map[string]string{
		"address.city": {"eq": "Paris"},
	}

	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatal("should match nested field")
	}
}

func TestApplyIncludesResolves(t *testing.T) {
	orig := api_storage.ReadEntityByIdFunc
	defer func() { api_storage.ReadEntityByIdFunc = orig }()

	api_storage.ReadEntityByIdFunc = func(entity, id string) map[string]any {
		if entity == "user" && id == "u1" {
			return map[string]any{"id": "u1", "name": "Alice"}
		}
		return nil
	}

	data := []map[string]any{
		{
			"id":     "p1",
			"title":  "Post 1",
			"author": map[string]any{"@entity": "user", "id": "u1"},
		},
	}

	result := api_storage.ApplyIncludes(data, "author")

	author, ok := result[0]["author"].(map[string]any)
	if !ok {
		t.Fatalf("author should be resolved map, got %T", result[0]["author"])
	}

	if author["name"] != "Alice" {
		t.Fatalf("expected author name=Alice, got %v", author["name"])
	}
}

func TestApplyIncludesAll(t *testing.T) {
	orig := api_storage.ReadEntityByIdFunc
	defer func() { api_storage.ReadEntityByIdFunc = orig }()

	api_storage.ReadEntityByIdFunc = func(entity, id string) map[string]any {
		if entity == "user" && id == "u1" {
			return map[string]any{"id": "u1", "name": "Alice"}
		}
		return nil
	}

	data := []map[string]any{
		{
			"id":     "p1",
			"title":  "Post 1",
			"author": map[string]any{"@entity": "user", "id": "u1"},
		},
	}

	result := api_storage.ApplyIncludes(data, "all")

	author, ok := result[0]["author"].(map[string]any)
	if !ok {
		t.Fatalf("author should be resolved map, got %T", result[0]["author"])
	}

	if author["name"] != "Alice" {
		t.Fatalf("expected author name=Alice, got %v", author["name"])
	}
}

func TestApplyIncludesNested(t *testing.T) {
	orig := api_storage.ReadEntityByIdFunc
	defer func() { api_storage.ReadEntityByIdFunc = orig }()

	api_storage.ReadEntityByIdFunc = func(entity, id string) map[string]any {
		switch {
		case entity == "user" && id == "u1":
			return map[string]any{
				"id":      "u1",
				"name":    "Alice",
				"profile": map[string]any{"@entity": "profile", "id": "pr1"},
			}
		case entity == "profile" && id == "pr1":
			return map[string]any{"id": "pr1", "bio": "Developer"}
		}
		return nil
	}

	data := []map[string]any{
		{
			"id":     "p1",
			"title":  "Post 1",
			"author": map[string]any{"@entity": "user", "id": "u1"},
		},
	}

	result := api_storage.ApplyIncludes(data, "author.profile")

	author, ok := result[0]["author"].(map[string]any)
	if !ok {
		t.Fatalf("author should be resolved, got %T", result[0]["author"])
	}

	if author["name"] != "Alice" {
		t.Fatalf("expected author name=Alice, got %v", author["name"])
	}

	profile, ok := author["profile"].(map[string]any)
	if !ok {
		t.Fatalf("profile should be resolved, got %T", author["profile"])
	}

	if profile["bio"] != "Developer" {
		t.Fatalf("expected bio=Developer, got %v", profile["bio"])
	}
}

func TestExtractAutoIncludes(t *testing.T) {
	filters := map[string]map[string]string{
		"author.name": {"eq": "Alice"},
		"tags":        {"contains": "go"},
	}

	result := api_storage.ExtractAutoIncludes(filters)
	if result != "author" {
		t.Fatalf("expected 'author', got %v", result)
	}
}

func TestMergeIncludesDedup(t *testing.T) {
	if api_storage.MergeIncludes("a,b", "c") != "a,b,c" {
		t.Fatal("merge failed")
	}

	if api_storage.MergeIncludes("a,b", "a") != "a,b" {
		t.Fatal("should deduplicate")
	}

	if api_storage.MergeIncludes("", "a") != "a" {
		t.Fatal("empty merge failed")
	}

	if api_storage.MergeIncludes("a", "") != "a" {
		t.Fatal("empty merge failed")
	}
}

func TestListEntitiesWithSearch(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("article", map[string]any{"id": "a1", "title": "Go tutorial"})
	api_storage.WriteEntity("article", map[string]any{"id": "a2", "title": "Rust guide"})
	api_storage.WriteEntity("article", map[string]any{"id": "a3", "title": "Go patterns"})

	result := api_storage.ListEntities("article", 0, 0, "", true, nil, "*Go*", "")
	if len(result) != 2 {
		t.Fatalf("search should return 2 results, got %d", len(result))
	}

	ids := map[string]bool{}
	for _, e := range result {
		ids[e["id"].(string)] = true
	}

	if !ids["a1"] || !ids["a3"] {
		t.Fatalf("expected a1 and a3, got %v", ids)
	}
}

func TestListEntitiesWithFilters(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("item", map[string]any{"id": "i1", "name": "alpha", "price": float64(10)})
	api_storage.WriteEntity("item", map[string]any{"id": "i2", "name": "beta", "price": float64(20)})
	api_storage.WriteEntity("item", map[string]any{"id": "i3", "name": "gamma", "price": float64(30)})

	filters := map[string]map[string]string{
		"price": {"gte": "20"},
	}

	result := api_storage.ListEntities("item", 0, 0, "", true, filters, "", "")
	if len(result) != 2 {
		t.Fatalf("expected 2 results, got %d", len(result))
	}
}

func TestListEntitiesWithPagination(t *testing.T) {
	initTestStore(t)

	for i := 0; i < 10; i++ {
		api_storage.WriteEntity("page", map[string]any{
			"id":   "p" + string(rune('0'+i)),
			"name": "item",
		})
	}

	result := api_storage.ListEntities("page", 3, 0, "", true, nil, "", "")
	if len(result) != 3 {
		t.Fatalf("limit 3 should return 3, got %d", len(result))
	}

	result = api_storage.ListEntities("page", 3, 5, "", true, nil, "", "")
	if len(result) != 3 {
		t.Fatalf("limit 3 offset 5 should return 3, got %d", len(result))
	}
}

func TestFiltersMatchEntityBoolean(t *testing.T) {
	entity := map[string]any{
		"id":     "1",
		"active": true,
	}

	if !api_storage.FiltersMatchEntity(entity, map[string]map[string]string{
		"active": {"eq": "true"},
	}) {
		t.Fatal("should match boolean true")
	}

	if api_storage.FiltersMatchEntity(entity, map[string]map[string]string{
		"active": {"eq": "false"},
	}) {
		t.Fatal("should not match boolean false")
	}
}

func TestFiltersMatchEntityArrayAllParity(t *testing.T) {
	entity := map[string]any{
		"id":   "1",
		"tags": []any{"go", "rust", "python"},
	}

	if !api_storage.FiltersMatchEntity(entity, map[string]map[string]string{
		"tags": {"all": "go,rust"},
	}) {
		t.Fatal("should match: all of [go,rust] in array")
	}

	if api_storage.FiltersMatchEntity(entity, map[string]map[string]string{
		"tags": {"all": "go,java"},
	}) {
		t.Fatal("should not match: java not in array")
	}
}

func TestFiltersMatchEntityArrayAnyParity(t *testing.T) {
	entity := map[string]any{
		"id":   "1",
		"tags": []any{"go", "rust"},
	}

	if !api_storage.FiltersMatchEntity(entity, map[string]map[string]string{
		"tags": {"any": "go,java"},
	}) {
		t.Fatal("should match: go is in array")
	}

	if api_storage.FiltersMatchEntity(entity, map[string]map[string]string{
		"tags": {"any": "java,c++"},
	}) {
		t.Fatal("should not match: neither java nor c++ in array")
	}
}

func TestFiltersMatchEntityArrayNoneParity(t *testing.T) {
	entity := map[string]any{
		"id":   "1",
		"tags": []any{"go", "rust"},
	}

	if !api_storage.FiltersMatchEntity(entity, map[string]map[string]string{
		"tags": {"none": "java,c++"},
	}) {
		t.Fatal("should match: neither java nor c++ in array")
	}

	if api_storage.FiltersMatchEntity(entity, map[string]map[string]string{
		"tags": {"none": "go,java"},
	}) {
		t.Fatal("should not match: go is in array")
	}
}
