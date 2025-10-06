package cache_test

import (
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/cache"
)

func TestCacheSetGet(t *testing.T) {
	cache.InitCache(10 * time.Second)
	entity := "users"
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "")
	value := []byte(`{"id":"1","name":"Alice"}`)
	cache.CacheStore.Set(entity, hash, value)
	got := cache.CacheStore.Get(entity, hash)
	if string(got) != string(value) {
		t.Fatalf("got %s, want %s", got, value)
	}
}

func TestCacheGetNotExist(t *testing.T) {
	cache.InitCache(10 * time.Second)
	entity := "users"
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "")
	got := cache.CacheStore.Get(entity, hash)
	if got != nil {
		t.Fatalf("expected nil, got %s", got)
	}
}

func TestCacheInvalidHashLength(t *testing.T) {
	cache.InitCache(10 * time.Second)
	entity := "users"
	got := cache.CacheStore.Get(entity, []byte("short"))
	if got != nil {
		t.Fatalf("expected nil for invalid hash length")
	}
	cache.CacheStore.Set(entity, []byte("short"), []byte("x"))
}

func TestCachePurge(t *testing.T) {
	cache.InitCache(10 * time.Second)
	entity := "users"
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "")
	value := []byte(`{"id":"1"}`)
	cache.CacheStore.Set(entity, hash, value)
	cache.CacheStore.Purge(entity)
	got := cache.CacheStore.Get(entity, hash)
	if got != nil {
		t.Fatalf("expected nil after purge, got %s", got)
	}
}

func TestHashQueryDifferentInputs(t *testing.T) {
	h1 := cache.HashQuery("users", 10, 0, "name", true, nil, "")
	h2 := cache.HashQuery("users", 20, 0, "name", true, nil, "")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for different limits")
	}
	h3 := cache.HashQuery("users", 10, 5, "name", true, nil, "")
	if string(h1) == string(h3) {
		t.Fatalf("expected different hashes for different offsets")
	}
	h4 := cache.HashQuery("users", 10, 0, "name", false, nil, "")
	if string(h1) == string(h4) {
		t.Fatalf("expected different hashes for ascending vs descending")
	}
	h5 := cache.HashQuery("users", 10, 0, "age", true, nil, "")
	if string(h1) == string(h5) {
		t.Fatalf("expected different hashes for different sort fields")
	}
}

func TestHashQueryWithFiltersStringOps(t *testing.T) {
	f1 := map[string]map[string]string{
		"name": {"eq": "Alice"},
	}
	f2 := map[string]map[string]string{
		"name": {"neq": "Alice"},
	}
	f3 := map[string]map[string]string{
		"name": {"eq": "Bob"},
	}
	h1 := cache.HashQuery("users", 10, 0, "name", true, f1, "")
	h2 := cache.HashQuery("users", 10, 0, "name", true, f2, "")
	h3 := cache.HashQuery("users", 10, 0, "name", true, f3, "")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for eq vs neq")
	}
	if string(h1) == string(h3) {
		t.Fatalf("expected different hashes for different eq values")
	}
}

func TestHashQueryWithFiltersNumericOps(t *testing.T) {
	f1 := map[string]map[string]string{
		"age": {"lt": "30"},
	}
	f2 := map[string]map[string]string{
		"age": {"lte": "30"},
	}
	f3 := map[string]map[string]string{
		"age": {"gt": "30"},
	}
	f4 := map[string]map[string]string{
		"age": {"gte": "30"},
	}
	h1 := cache.HashQuery("users", 10, 0, "age", true, f1, "")
	h2 := cache.HashQuery("users", 10, 0, "age", true, f2, "")
	h3 := cache.HashQuery("users", 10, 0, "age", true, f3, "")
	h4 := cache.HashQuery("users", 10, 0, "age", true, f4, "")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for lt vs lte")
	}
	if string(h1) == string(h3) {
		t.Fatalf("expected different hashes for lt vs gt")
	}
	if string(h2) == string(h4) {
		t.Fatalf("expected different hashes for lte vs gte")
	}
}

func TestHashQueryWithMultipleFilters(t *testing.T) {
	f1 := map[string]map[string]string{
		"name": {"eq": "Alice"},
		"age":  {"gte": "20"},
	}
	f2 := map[string]map[string]string{
		"name": {"eq": "Alice"},
		"age":  {"gte": "30"},
	}
	h1 := cache.HashQuery("users", 10, 0, "name", true, f1, "")
	h2 := cache.HashQuery("users", 10, 0, "name", true, f2, "")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for different filter values on age")
	}
}

func TestHashQueryWithNestedFilters(t *testing.T) {
	f1 := map[string]map[string]string{
		"profile.name": {"eq": "Alice"},
	}
	f2 := map[string]map[string]string{
		"profile.age": {"lt": "40"},
	}
	h1 := cache.HashQuery("users", 10, 0, "profile.name", true, f1, "")
	h2 := cache.HashQuery("users", 10, 0, "profile.age", true, f2, "")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for nested filters with different fields")
	}
}

func TestHashQueryWithNestedSort(t *testing.T) {
	h1 := cache.HashQuery("users", 10, 0, "profile.name", true, nil, "")
	h2 := cache.HashQuery("users", 10, 0, "profile.name", false, nil, "")
	h3 := cache.HashQuery("users", 10, 0, "profile.age", true, nil, "")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for asc vs desc on nested field")
	}
	if string(h1) == string(h3) {
		t.Fatalf("expected different hashes for different nested sort fields")
	}
}

func TestCacheExpiration(t *testing.T) {
	cache.InitCache(100 * time.Millisecond)
	entity := "users"
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "")
	value := []byte(`{"id":"1","name":"Alice"}`)
	cache.CacheStore.Set(entity, hash, value)
	time.Sleep(200 * time.Millisecond)
	cache.CacheStore.CleanExpired()
	got := cache.CacheStore.Get(entity, hash)
	if got != nil {
		t.Fatalf("expected nil after expiration, got %s", got)
	}
}

func TestCacheSetGetById(t *testing.T) {
	cache.InitCache(10 * time.Second)
	entity := "users"
	id := "123"
	value := []byte(`{"id":"123","name":"Bob"}`)
	cache.CacheStore.SetById(entity, id, value)
	got := cache.CacheStore.GetById(entity, id)
	if string(got) != string(value) {
		t.Fatalf("got %s, want %s", got, value)
	}
}

func TestCacheGetByIdNotExist(t *testing.T) {
	cache.InitCache(10 * time.Second)
	entity := "users"
	id := "notfound"
	got := cache.CacheStore.GetById(entity, id)
	if got != nil {
		t.Fatalf("expected nil, got %s", got)
	}
}

func TestCacheExpirationById(t *testing.T) {
	cache.InitCache(100 * time.Millisecond)
	entity := "users"
	id := "456"
	value := []byte(`{"id":"456","name":"Charlie"}`)
	cache.CacheStore.SetById(entity, id, value)
	time.Sleep(200 * time.Millisecond)
	cache.CacheStore.CleanExpired()
	got := cache.CacheStore.GetById(entity, id)
	if got != nil {
		t.Fatalf("expected nil after expiration, got %s", got)
	}
}

func TestCacheCleanExpiredRemovesBothDataAndIds(t *testing.T) {
	cache.InitCache(10 * time.Millisecond)
	entity := "users"
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "")
	cache.CacheStore.Set(entity, hash, []byte("val"))
	cache.CacheStore.SetById(entity, "id1", []byte("val"))
	time.Sleep(20 * time.Millisecond)
	cache.CacheStore.CleanExpired()
	if v := cache.CacheStore.Get(entity, hash); v != nil {
		t.Fatalf("expected nil after CleanExpired, got %s", v)
	}
	if v := cache.CacheStore.GetById(entity, "id1"); v != nil {
		t.Fatalf("expected nil after CleanExpired, got %s", v)
	}
}
