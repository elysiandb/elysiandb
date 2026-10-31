package api_test

import (
	"reflect"
	"sort"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
)

func newSimulation() *api_storage.WriteSimulation {
	return api_storage.NewWriteSimulation(api_storage.GetEntitySchema, api_storage.ReadEntityById)
}

func enableSchema(strict bool) {
	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = strict
}

func TestWriteSimulation_TracksSchemaEvolution(t *testing.T) {
	initTestStore(t)
	enableSchema(false)

	sim := newSimulation()
	if errs := sim.Write("val", map[string]any{"id": "x1", "a": "s"}); len(errs) != 0 {
		t.Fatalf("unexpected errors %v", errs)
	}

	if errs := sim.Write("val", map[string]any{"id": "x2", "a": 5.0}); len(errs) != 1 || errs[0].Field != "a" {
		t.Fatalf("expected the second write to fail against the simulated schema, got %v", errs)
	}

	if api_storage.EntityTypeExists("val") || api_storage.ReadEntityById("val", "x1") != nil {
		t.Fatalf("a simulation must not write anything")
	}
}

func TestWriteSimulation_UsesStoredSchema(t *testing.T) {
	initTestStore(t)
	enableSchema(false)
	api_storage.WriteEntity("val", map[string]any{"id": "x0", "a": "s"})

	if errs := newSimulation().Write("val", map[string]any{"id": "x1", "a": 1.0}); len(errs) != 1 {
		t.Fatalf("expected the stored schema to be used, got %v", errs)
	}
}

func TestWriteSimulation_TracksExistence(t *testing.T) {
	initTestStore(t)
	enableSchema(false)
	api_storage.WriteEntity("acct", map[string]any{"id": "B", "balance": 100.0})

	sim := newSimulation()
	if !sim.Update("acct", "B", map[string]any{"balance": 1.0}) {
		t.Fatalf("expected an update on an existing document to succeed")
	}

	sim.Delete("acct", "B")
	if sim.Update("acct", "B", map[string]any{"balance": 0.0}) {
		t.Fatalf("expected an update after a delete to fail")
	}

	if sim.Update("acct", "missing", map[string]any{"balance": 0.0}) {
		t.Fatalf("expected an update on a missing document to fail")
	}

	sim.Write("acct", map[string]any{"id": "B", "balance": 5.0})
	if !sim.Update("acct", "B", map[string]any{"balance": 6.0}) {
		t.Fatalf("expected an update after a write to succeed")
	}

	if got := api_storage.ReadEntityById("acct", "B")["balance"]; got != 100.0 {
		t.Fatalf("a simulation must not change stored documents, got %v", got)
	}
}

func TestWriteSimulation_TracksSubEntities(t *testing.T) {
	initTestStore(t)
	enableSchema(false)

	data := map[string]any{
		"id":     "p1",
		"title":  "T",
		"author": map[string]any{"@entity": "writer", "id": "w1", "name": "N"},
	}

	sim := newSimulation()
	sim.Write("post", data)

	if !sim.Update("writer", "w1", map[string]any{"name": "M"}) {
		t.Fatalf("expected the nested writer to exist in the simulation")
	}

	if _, nested := data["author"].(map[string]any)["name"]; !nested {
		t.Fatalf("a simulation must not rewrite the payload")
	}
}

func TestNestedEntityTypes(t *testing.T) {
	data := map[string]any{
		"author": map[string]any{"@entity": "author", "id": "a1", "job": map[string]any{"@entity": "job", "id": "j1"}},
		"tags":   []any{map[string]any{"@entity": "tag", "id": "t1"}, map[string]any{"@entity": "author", "id": "a2"}, "plain"},
		"meta":   map[string]any{"x": 1.0},
	}

	got := api_storage.NestedEntityTypes(data)
	sort.Strings(got)

	if !reflect.DeepEqual(got, []string{"author", "job", "tag"}) {
		t.Fatalf("unexpected nested entity types %v", got)
	}

	list := api_storage.NestedEntityTypes([]any{map[string]any{"a": map[string]any{"@entity": "x", "id": "1"}}})
	if !reflect.DeepEqual(list, []string{"x"}) {
		t.Fatalf("expected nested types inside a list, got %v", list)
	}

	if got := api_storage.NestedEntityTypes(map[string]any{"a": 1.0}); len(got) != 0 {
		t.Fatalf("expected no nested types, got %v", got)
	}
}

func TestWriteSimulation_SubEntitiesFollowTheStorageRules(t *testing.T) {
	initTestStore(t)
	enableSchema(false)
	api_storage.WriteEntity("author", map[string]any{"id": "x", "age": 30.0})

	data := map[string]any{"id": "b", "author": map[string]any{"@entity": "author", "id": "z", "age": "thirty"}}
	if errs := newSimulation().Write("book", data); len(errs) != 0 {
		t.Fatalf("expected the parent write to pass, got %v", errs)
	}

	sim := newSimulation()
	sim.Write("book", data)
	if !sim.Update("author", "z", map[string]any{"name": "Z"}) {
		t.Fatalf("expected the nested author to be written despite its inferred type")
	}

	if errs := newSimulation().Write("author", map[string]any{"id": "w", "age": "thirty"}); len(errs) != 1 {
		t.Fatalf("expected a direct write to be checked against the inferred schema, got %v", errs)
	}
}
