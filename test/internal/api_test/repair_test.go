package api_test

import (
	"slices"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func listIDs(entity string) []string {
	ids := []string{}
	for _, e := range api_storage.ListEntities(entity, 0, 0, "", true, nil, "", "") {
		if id, ok := e["id"].(string); ok {
			ids = append(ids, id)
		}
	}
	slices.Sort(ids)

	return ids
}

func TestRepairIndexesRebuildsWhatAResetOfAnEarlierVersionRemoved(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("books", map[string]any{"id": "b1", "title": "Dune"})
	api_storage.WriteEntity("books", map[string]any{"id": "b2", "title": "Emma"})
	api_storage.WriteEntity("authors", map[string]any{"id": "a1", "name": "Herbert"})

	storage.DeleteByWildcardKey(globals.ApiKeyPrefix + "*")

	if got := api_storage.ListEntityTypes(); len(got) != 0 {
		t.Fatalf("the KV indexes should be gone, got %v", got)
	}

	repair := api_storage.RepairIndexes()
	if !repair.Changed() || repair.IDsAdded != 3 || repair.IDsRemoved != 0 {
		t.Fatalf("unexpected repair %+v", repair)
	}

	for _, entity := range []string{"books", "authors"} {
		if !slices.Contains(repair.EntityTypes, entity) || !api_storage.EntityTypeExists(entity) {
			t.Fatalf("%s should be restored: %+v", entity, repair)
		}
	}

	if got := listIDs("books"); !slices.Equal(got, []string{"b1", "b2"}) {
		t.Fatalf("books should be listed again, got %v", got)
	}
	if got := api_storage.CountEntities("authors"); got != 1 {
		t.Fatalf("authors should be counted again, got %d", got)
	}

	if again := api_storage.RepairIndexes(); again.Changed() {
		t.Fatalf("a second repair must change nothing, got %+v", again)
	}
}

func TestRepairIndexesKeepsTheOrderAndDropsUnknownIDs(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("books", map[string]any{"id": "z", "title": "first"})
	api_storage.WriteEntity("books", map[string]any{"id": "a", "title": "second"})
	api_storage.WriteEntity("books", map[string]any{"id": "m", "title": "third"})

	key := globals.ApiEntityIndexIdKey("books")
	if err := storage.PutKeyValue(key, []byte("z\nghost\nz\nm")); err != nil {
		t.Fatal(err)
	}

	repair := api_storage.RepairIndexes()
	if repair.IDsAdded != 1 || repair.IDsRemoved != 2 || len(repair.EntityTypes) != 0 {
		t.Fatalf("unexpected repair %+v", repair)
	}

	raw, _ := storage.GetByKey(key)
	if string(raw) != "z\nm\na" {
		t.Fatalf("existing order must be kept and missing ids appended, got %q", raw)
	}
}

func TestRepairIndexesWithoutChanges(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("books", map[string]any{"id": "b1", "title": "Dune"})

	if repair := api_storage.RepairIndexes(); repair.Changed() {
		t.Fatalf("a consistent store needs no repair, got %+v", repair)
	}
}
