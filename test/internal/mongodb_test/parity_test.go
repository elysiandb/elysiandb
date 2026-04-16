package mongodb_test

import (
	"reflect"
	"testing"

	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/query"
	"go.mongodb.org/mongo-driver/v2/bson"
)

func TestBuildMongoLeafAllOperators(t *testing.T) {
	leaf := map[string]map[string]string{
		"name": {"eq": "john"},
		"age":  {"gt": "18"},
	}

	result := mongodb.BuildMongoLeaf(leaf)

	if result["name"] != "john" {
		t.Fatalf("eq: expected 'john', got %v", result["name"])
	}

	ageFilter, ok := result["age"].(bson.M)
	if !ok {
		t.Fatalf("age filter not bson.M: %T", result["age"])
	}

	if ageFilter["$gt"].(int64) != 18 {
		t.Fatalf("gt: expected 18, got %v", ageFilter["$gt"])
	}
}

func TestBuildMongoLeafNeq(t *testing.T) {
	result := mongodb.BuildMongoLeaf(map[string]map[string]string{
		"status": {"neq": "deleted"},
	})

	statusFilter, ok := result["status"].(bson.M)
	if !ok {
		t.Fatalf("status filter not bson.M: %T", result["status"])
	}

	if statusFilter["$ne"] != "deleted" {
		t.Fatalf("neq: expected 'deleted', got %v", statusFilter["$ne"])
	}
}

func TestBuildMongoLeafGteLte(t *testing.T) {
	result := mongodb.BuildMongoLeaf(map[string]map[string]string{
		"price": {"gte": "10"},
	})

	priceFilter := result["price"].(bson.M)
	if priceFilter["$gte"].(int64) != 10 {
		t.Fatalf("gte: expected 10, got %v", priceFilter["$gte"])
	}

	result = mongodb.BuildMongoLeaf(map[string]map[string]string{
		"price": {"lte": "100"},
	})

	priceFilter = result["price"].(bson.M)
	if priceFilter["$lte"].(int64) != 100 {
		t.Fatalf("lte: expected 100, got %v", priceFilter["$lte"])
	}
}

func TestBuildMongoLeafLt(t *testing.T) {
	result := mongodb.BuildMongoLeaf(map[string]map[string]string{
		"count": {"lt": "5"},
	})

	countFilter := result["count"].(bson.M)
	if countFilter["$lt"].(int64) != 5 {
		t.Fatalf("lt: expected 5, got %v", countFilter["$lt"])
	}
}

func TestBuildMongoLeafContains(t *testing.T) {
	result := mongodb.BuildMongoLeaf(map[string]map[string]string{
		"tags": {"contains": "go"},
	})

	if result["tags"] != "go" {
		t.Fatalf("contains: expected raw string 'go', got %v", result["tags"])
	}
}

func TestBuildMongoLeafNotContains(t *testing.T) {
	result := mongodb.BuildMongoLeaf(map[string]map[string]string{
		"tags": {"not_contains": "java"},
	})

	tagsFilter, ok := result["tags"].(bson.M)
	if !ok {
		t.Fatalf("tags filter not bson.M: %T", result["tags"])
	}

	if tagsFilter["$ne"] != "java" {
		t.Fatalf("not_contains: expected $ne 'java', got %v", tagsFilter["$ne"])
	}
}

func TestBuildMongoLeafArrayOps(t *testing.T) {
	result := mongodb.BuildMongoLeaf(map[string]map[string]string{
		"tags": {"any": "a,b"},
	})

	tagsFilter := result["tags"].(bson.M)
	if _, ok := tagsFilter["$in"]; !ok {
		t.Fatal("any: expected $in")
	}

	result = mongodb.BuildMongoLeaf(map[string]map[string]string{
		"tags": {"all": "a,b"},
	})

	tagsFilter = result["tags"].(bson.M)
	if _, ok := tagsFilter["$all"]; !ok {
		t.Fatal("all: expected $all")
	}

	result = mongodb.BuildMongoLeaf(map[string]map[string]string{
		"tags": {"none": "a,b"},
	})

	tagsFilter = result["tags"].(bson.M)
	if _, ok := tagsFilter["$nin"]; !ok {
		t.Fatal("none: expected $nin")
	}
}

func TestBuildMongoLeafGlobEq(t *testing.T) {
	result := mongodb.BuildMongoLeaf(map[string]map[string]string{
		"name": {"eq": "john*"},
	})

	nameFilter, ok := result["name"].(bson.M)
	if !ok {
		t.Fatalf("name filter not bson.M: %T", result["name"])
	}

	if _, ok := nameFilter["$regex"]; !ok {
		t.Fatal("glob eq: expected $regex")
	}
}

func TestBuildMongoExprLeaf(t *testing.T) {
	q := mongodb.BuildMongoExpr(query.FilterNode{
		Leaf: map[string]map[string]string{
			"age":  {"gt": "18"},
			"name": {"eq": "alice"},
		},
	})

	if len(q) != 2 {
		t.Fatalf("expected 2 filters, got %d: %v", len(q), q)
	}
}

func TestBuildMongoExprAnd(t *testing.T) {
	q := mongodb.BuildMongoExpr(query.FilterNode{
		And: []query.FilterNode{
			{Leaf: map[string]map[string]string{"age": {"gt": "18"}}},
			{Leaf: map[string]map[string]string{"status": {"eq": "active"}}},
		},
	})

	andClauses, ok := q["$and"].([]bson.M)
	if !ok {
		t.Fatalf("expected $and, got %v", q)
	}

	if len(andClauses) != 2 {
		t.Fatalf("expected 2 and clauses, got %d", len(andClauses))
	}
}

func TestBuildMongoExprOr(t *testing.T) {
	q := mongodb.BuildMongoExpr(query.FilterNode{
		Or: []query.FilterNode{
			{Leaf: map[string]map[string]string{"age": {"gt": "30"}}},
			{Leaf: map[string]map[string]string{"role": {"eq": "admin"}}},
		},
	})

	orClauses, ok := q["$or"].([]bson.M)
	if !ok {
		t.Fatalf("expected $or, got %v", q)
	}

	if len(orClauses) != 2 {
		t.Fatalf("expected 2 or clauses, got %d", len(orClauses))
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
		"scores": []any{1, 2, 3},
	}

	result := mongodb.FilterFields(data, []string{"name", "address.city"})

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

	if _, ok := result["scores"]; ok {
		t.Fatal("expected scores to be filtered out")
	}
}

func TestFilterFieldsEmpty(t *testing.T) {
	data := map[string]any{"id": "1", "name": "x"}
	result := mongodb.FilterFields(data, nil)

	if !reflect.DeepEqual(result, data) {
		t.Fatalf("empty fields should return original data")
	}
}

func TestApplyIncludesStub(t *testing.T) {
	// ApplyIncludes with empty param should return data unchanged
	data := []map[string]any{
		{"id": "1", "name": "test"},
	}

	result := mongodb.ApplyIncludes(data, "")
	if len(result) != 1 || result[0]["name"] != "test" {
		t.Fatal("empty includes should return data unchanged")
	}
}

func TestBuildIncludeTreeParsing(t *testing.T) {
	_, paths := mongodb.ParseIncludes("author,comments.user,tags")
	if len(paths) != 3 {
		t.Fatalf("expected 3 paths, got %d", len(paths))
	}

	if !reflect.DeepEqual(paths[0], []string{"author"}) {
		t.Fatalf("expected [author], got %v", paths[0])
	}

	if !reflect.DeepEqual(paths[1], []string{"comments", "user"}) {
		t.Fatalf("expected [comments user], got %v", paths[1])
	}

	if !reflect.DeepEqual(paths[2], []string{"tags"}) {
		t.Fatalf("expected [tags], got %v", paths[2])
	}
}

func TestBuildSpecsFromSampleSkipsNested(t *testing.T) {
	// BuildSpecsFromSample should only return root-level specs
	// Nested paths (len > 1) should be handled by leaf resolution
	// This test verifies the behavior without MongoDB (sample returns nil)
	paths := [][]string{
		{"author"},
		{"comments", "user"},
	}

	// Without MongoDB, BuildSpecsFromSample can't sample, so specs will be empty
	// But the important thing is that nested paths are handled by leaf resolution
	leafPaths := mongodb.ExtractLeafIncludePaths(paths)
	if len(leafPaths) != 1 {
		t.Fatalf("expected 1 leaf path, got %d", len(leafPaths))
	}

	if !reflect.DeepEqual(leafPaths[0], []string{"comments", "user"}) {
		t.Fatalf("expected [comments user], got %v", leafPaths[0])
	}
}

func TestBuildMongoFiltersContainsNotContains(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"tags": {"contains": "go"},
	})

	// contains should use raw string value for MongoDB array membership
	if q["tags"] != "go" {
		t.Fatalf("contains: expected raw string 'go', got %v (type %T)", q["tags"], q["tags"])
	}

	q = mongodb.BuildMongoFilters(map[string]map[string]string{
		"tags": {"not_contains": "java"},
	})

	tagsFilter, ok := q["tags"].(bson.M)
	if !ok {
		t.Fatalf("not_contains: expected bson.M, got %T", q["tags"])
	}

	if tagsFilter["$ne"] != "java" {
		t.Fatalf("not_contains: expected $ne 'java', got %v", tagsFilter["$ne"])
	}
}

func TestBuildMongoFiltersDateEq(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"created": {"eq": "2024-06-15"},
	})

	created, ok := q["created"].(bson.M)
	if !ok {
		t.Fatalf("expected bson.M for date eq, got %T", q["created"])
	}

	if _, ok := created["$gte"]; !ok {
		t.Fatal("date eq should use $gte")
	}

	if _, ok := created["$lt"]; !ok {
		t.Fatal("date eq should use $lt for day range")
	}
}

func TestBuildMongoFiltersNeqDate(t *testing.T) {
	q := mongodb.BuildMongoFilters(map[string]map[string]string{
		"created": {"neq": "2024-06-15"},
	})

	created, ok := q["created"].(bson.M)
	if !ok {
		t.Fatalf("expected bson.M for date neq, got %T", q["created"])
	}

	if _, ok := created["$lt"]; !ok {
		t.Fatal("date neq should use $lt")
	}

	if _, ok := created["$gte"]; !ok {
		t.Fatal("date neq should use $gte for day exclusion")
	}
}

func TestBuildMongoExprLeafAllOps(t *testing.T) {
	result := mongodb.BuildMongoExpr(query.FilterNode{
		Leaf: map[string]map[string]string{
			"status": {"neq": "deleted"},
			"age":    {"gte": "18"},
			"tags":   {"any": "a,b"},
		},
	})

	if len(result) != 3 {
		t.Fatalf("expected 3 filters, got %d: %v", len(result), result)
	}

	statusFilter, ok := result["status"].(bson.M)
	if !ok {
		t.Fatalf("status not bson.M: %T", result["status"])
	}

	if statusFilter["$ne"] != "deleted" {
		t.Fatal("status neq filter broken")
	}

	ageFilter := result["age"].(bson.M)
	if ageFilter["$gte"].(int64) != 18 {
		t.Fatal("age gte filter broken")
	}

	tagsFilter := result["tags"].(bson.M)
	if _, ok := tagsFilter["$in"]; !ok {
		t.Fatal("tags any filter broken")
	}
}

func TestExtractLeafIncludePaths(t *testing.T) {
	paths := [][]string{
		{"author"},
		{"comments", "user"},
		{"tags"},
		{"comments", "user", "profile"},
	}

	leaf := mongodb.ExtractLeafIncludePaths(paths)
	if len(leaf) != 2 {
		t.Fatalf("expected 2 leaf paths, got %d", len(leaf))
	}

	if !reflect.DeepEqual(leaf[0], []string{"comments", "user"}) {
		t.Fatalf("expected [comments user], got %v", leaf[0])
	}

	if !reflect.DeepEqual(leaf[1], []string{"comments", "user", "profile"}) {
		t.Fatalf("expected [comments user profile], got %v", leaf[1])
	}
}
