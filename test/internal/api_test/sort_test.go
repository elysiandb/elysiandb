package api_test

import (
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func initSortTestStore(t *testing.T) {
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

func TestGetSortedEntityIdsByField_IntAscDesc(t *testing.T) {
	initSortTestStore(t)

	entity := "people"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "p1", "age": 30})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "p2", "age": 22})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "p3", "age": 40})

	asc := api_storage.GetSortedEntityIdsByField(entity, "age", true)
	desc := api_storage.GetSortedEntityIdsByField(entity, "age", false)

	wantAsc := []string{"p2", "p1", "p3"}
	wantDesc := []string{"p3", "p1", "p2"}

	if !equalSlices(asc, wantAsc) {
		t.Fatalf("age asc = %v, want %v", asc, wantAsc)
	}
	if !equalSlices(desc, wantDesc) {
		t.Fatalf("age desc = %v, want %v", desc, wantDesc)
	}
}

func TestGetSortedEntityIdsByField_FloatAscDesc(t *testing.T) {
	initSortTestStore(t)

	entity := "scores"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "s1", "score": 1.5})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "s2", "score": 3.0})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "s3", "score": 2.2})

	asc := api_storage.GetSortedEntityIdsByField(entity, "score", true)
	desc := api_storage.GetSortedEntityIdsByField(entity, "score", false)

	wantAsc := []string{"s1", "s3", "s2"}
	wantDesc := []string{"s2", "s3", "s1"}

	if !equalSlices(asc, wantAsc) {
		t.Fatalf("score asc = %v, want %v", asc, wantAsc)
	}
	if !equalSlices(desc, wantDesc) {
		t.Fatalf("score desc = %v, want %v", desc, wantDesc)
	}
}

func TestGetSortedEntityIdsByField_StringAscDesc(t *testing.T) {
	initSortTestStore(t)

	entity := "cities"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c1", "name": "Paris"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c2", "name": "Berlin"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "c3", "name": "Tokyo"})

	asc := api_storage.GetSortedEntityIdsByField(entity, "name", true)
	desc := api_storage.GetSortedEntityIdsByField(entity, "name", false)

	wantAsc := []string{"c2", "c1", "c3"}
	wantDesc := []string{"c3", "c1", "c2"}

	if !equalSlices(asc, wantAsc) {
		t.Fatalf("name asc = %v, want %v", asc, wantAsc)
	}
	if !equalSlices(desc, wantDesc) {
		t.Fatalf("name desc = %v, want %v", desc, wantDesc)
	}
}

func TestGetSortedEntityIdsByField_MixedTypesAndMissing(t *testing.T) {
	initSortTestStore(t)

	entity := "mixed"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "m1", "rank": 10})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "m2", "rank": 5})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "m3", "rank": "not-an-int"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "m4"})

	asc := api_storage.GetSortedEntityIdsByField(entity, "rank", true)
	desc := api_storage.GetSortedEntityIdsByField(entity, "rank", false)

	wantSet := map[string]bool{"m1": true, "m2": true, "m3": true, "m4": true}
	if !containsExactly(asc, wantSet) {
		t.Fatalf("asc should contain all ids, got %v", asc)
	}
	if !containsExactly(desc, wantSet) {
		t.Fatalf("desc should contain all ids, got %v", desc)
	}

	if idx(asc, "m2") > idx(asc, "m1") {
		t.Fatalf("expected m2 to come before m1 in asc, got %v", asc)
	}
	if idx(desc, "m1") > idx(desc, "m2") {
		t.Fatalf("expected m1 to come before m2 in desc, got %v", desc)
	}
}

func TestGetSortedEntityIdsByField_NestedFields(t *testing.T) {
	initSortTestStore(t)

	entity := "books"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "b1", "author": map[string]interface{}{"name": "Charles"}})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "b2", "author": map[string]interface{}{"name": "Alice"}})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "b3", "author": map[string]interface{}{"name": "Bob"}})

	asc := api_storage.GetSortedEntityIdsByField(entity, "author.name", true)
	desc := api_storage.GetSortedEntityIdsByField(entity, "author.name", false)

	wantAsc := []string{"b2", "b3", "b1"}
	wantDesc := []string{"b1", "b3", "b2"}

	if !equalSlices(asc, wantAsc) {
		t.Fatalf("nested author.name asc = %v, want %v", asc, wantAsc)
	}
	if !equalSlices(desc, wantDesc) {
		t.Fatalf("nested author.name desc = %v, want %v", desc, wantDesc)
	}
}

func TestGetSortedEntityIdsByField_DateAscDesc(t *testing.T) {
	initSortTestStore(t)

	entity := "events"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "e1", "date": "2023-01-01"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "e2", "date": "2022-12-31"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "e3", "date": "2023-01-02"})

	asc := api_storage.GetSortedEntityIdsByField(entity, "date", true)
	desc := api_storage.GetSortedEntityIdsByField(entity, "date", false)

	wantAsc := []string{"e2", "e1", "e3"}
	wantDesc := []string{"e3", "e1", "e2"}

	if !equalSlices(asc, wantAsc) {
		t.Fatalf("date asc = %v, want %v", asc, wantAsc)
	}
	if !equalSlices(desc, wantDesc) {
		t.Fatalf("date desc = %v, want %v", desc, wantDesc)
	}
}

func TestGetSortedEntityIdsByField_DateTimeAscDesc(t *testing.T) {
	initSortTestStore(t)

	entity := "events"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "e1", "date": "2023-01-01T10:00:00Z"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "e2", "date": "2022-12-31T23:59:59Z"})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "e3", "date": "2023-01-02T00:00:00Z"})

	asc := api_storage.GetSortedEntityIdsByField(entity, "date", true)
	desc := api_storage.GetSortedEntityIdsByField(entity, "date", false)

	wantAsc := []string{"e2", "e1", "e3"}
	wantDesc := []string{"e3", "e1", "e2"}

	if !equalSlices(asc, wantAsc) {
		t.Fatalf("date asc = %v, want %v", asc, wantAsc)
	}
	if !equalSlices(desc, wantDesc) {
		t.Fatalf("date desc = %v, want %v", desc, wantDesc)
	}
}

func TestGetSortedEntityIdsByField_NestedDateAscDesc(t *testing.T) {
	initSortTestStore(t)

	entity := "meetings"
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "m1", "schedule": map[string]interface{}{"start": "2023-01-02"}})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "m2", "schedule": map[string]interface{}{"start": "2022-12-31"}})
	api_storage.WriteEntity(entity, map[string]interface{}{"id": "m3", "schedule": map[string]interface{}{"start": "2023-01-01"}})

	asc := api_storage.GetSortedEntityIdsByField(entity, "schedule.start", true)
	desc := api_storage.GetSortedEntityIdsByField(entity, "schedule.start", false)

	wantAsc := []string{"m2", "m3", "m1"}
	wantDesc := []string{"m1", "m3", "m2"}

	if !equalSlices(asc, wantAsc) {
		t.Fatalf("nested date asc = %v, want %v", asc, wantAsc)
	}
	if !equalSlices(desc, wantDesc) {
		t.Fatalf("nested date desc = %v, want %v", desc, wantDesc)
	}
}

func equalSlices(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func containsExactly(got []string, want map[string]bool) bool {
	if len(got) != len(want) {
		return false
	}
	seen := make(map[string]bool, len(got))
	for _, id := range got {
		seen[id] = true
	}
	for k := range want {
		if !seen[k] {
			return false
		}
	}
	return true
}

func idx(arr []string, v string) int {
	for i, x := range arr {
		if x == v {
			return i
		}
	}
	return -1
}
