package cache_test

import (
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/cache"
)

func TestCacheSetGet(t *testing.T) {
	cache.InitCache(10 * time.Second)
	entity := "users"
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "", "", "", false, "alice")
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
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "", "", "", false, "alice")
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
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "", "", "", false, "alice")
	value := []byte(`{"id":"1"}`)
	cache.CacheStore.Set(entity, hash, value)
	cache.CacheStore.Purge(entity)
	got := cache.CacheStore.Get(entity, hash)
	if got != nil {
		t.Fatalf("expected nil after purge, got %s", got)
	}
}

func TestHashQueryDifferentInputs(t *testing.T) {
	h1 := cache.HashQuery("users", 10, 0, "name", true, nil, "", "", "", false, "alice")
	h2 := cache.HashQuery("users", 20, 0, "name", true, nil, "", "", "", false, "alice")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for different limits")
	}
	h3 := cache.HashQuery("users", 10, 5, "name", true, nil, "", "", "", false, "alice")
	if string(h1) == string(h3) {
		t.Fatalf("expected different hashes for different offsets")
	}
	h4 := cache.HashQuery("users", 10, 0, "name", false, nil, "", "", "", false, "alice")
	if string(h1) == string(h4) {
		t.Fatalf("expected different hashes for ascending vs descending")
	}
	h5 := cache.HashQuery("users", 10, 0, "age", true, nil, "", "", "", false, "alice")
	if string(h1) == string(h5) {
		t.Fatalf("expected different hashes for different sort fields")
	}
}

func TestHashQueryWithDifferentUsers(t *testing.T) {
	h1 := cache.HashQuery("users", 10, 0, "name", true, nil, "", "", "", false, "alice")
	h2 := cache.HashQuery("users", 10, 0, "name", true, nil, "", "", "", false, "bob")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for different usernames")
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
	h1 := cache.HashQuery("users", 10, 0, "name", true, f1, "", "", "", false, "alice")
	h2 := cache.HashQuery("users", 10, 0, "name", true, f2, "", "", "", false, "alice")
	h3 := cache.HashQuery("users", 10, 0, "name", true, f3, "", "", "", false, "alice")
	if string(h1) == string(h2) {
		t.Fatalf("expected different hashes for eq vs neq")
	}
	if string(h1) == string(h3) {
		t.Fatalf("expected different hashes for different eq values")
	}
}

func TestCacheExpiration(t *testing.T) {
	cache.InitCache(100 * time.Millisecond)
	entity := "users"
	hash := cache.HashQuery("users", 10, 0, "name", true, nil, "", "", "", false, "alice")
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
