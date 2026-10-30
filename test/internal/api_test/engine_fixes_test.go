package api_test

import (
	"encoding/json"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/schema"
)

func toJSON(t *testing.T, v any) string {
	t.Helper()

	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	return string(b)
}

func listIds(data []map[string]any) []string {
	ids := make([]string, 0, len(data))
	for _, d := range data {
		id, _ := d["id"].(string)
		ids = append(ids, id)
	}

	return ids
}

func TestArrayOfReferencesDoesNotOverwriteReferencedEntities(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("author", map[string]any{"id": "a1", "name": "Ada"})
	api_storage.WriteEntity("post", map[string]any{"id": "p1", "authors": []any{
		map[string]any{"@entity": "author", "id": "a1"},
		map[string]any{"@entity": "author", "id": "a2", "name": "Grace"},
	}})

	if got := toJSON(t, api_storage.ReadEntityById("author", "a1")); got != `{"id":"a1","name":"Ada"}` {
		t.Fatalf("referenced author was overwritten: %s", got)
	}

	if got := toJSON(t, api_storage.ReadEntityById("author", "a2")); got != `{"id":"a2","name":"Grace"}` {
		t.Fatalf("inline author was not created: %s", got)
	}

	if got := toJSON(t, api_storage.ReadEntityById("post", "p1")); got != `{"authors":[{"@entity":"author","id":"a1"},{"@entity":"author","id":"a2"}],"id":"p1"}` {
		t.Fatalf("unexpected post: %s", got)
	}
}

func TestMigrationKeepsEntitiesReachableThroughReferences(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("chain", map[string]any{"id": "x3", "title": "third", "next": map[string]any{"@entity": "chain", "id": "x2"}, "related": []any{map[string]any{"@entity": "chain", "id": "x1"}}})
	api_storage.WriteEntity("chain", map[string]any{"id": "x1", "title": "first", "related": []any{map[string]any{"@entity": "chain", "id": "x2"}}})
	api_storage.WriteEntity("chain", map[string]any{"id": "x2", "title": "second", "next": map[string]any{"@entity": "chain", "id": "x1"}})

	api_storage.WriteEntity("pair", map[string]any{"id": "y2", "title": "referenced first"})
	api_storage.WriteEntity("pair", map[string]any{"id": "y1", "title": "references", "related": []any{map[string]any{"@entity": "pair", "id": "y2"}}, "main": map[string]any{"@entity": "pair", "id": "y2"}})
	if err := api_storage.ExecuteMigrations([]api_storage.MigrationQuery{{Entity: "pair", Action: "set", Properties: map[string]any{"migrated": true}}}); err != nil {
		t.Fatalf("migration: %v", err)
	}
	if got := toJSON(t, api_storage.ReadEntityById("pair", "y2")); got != `{"id":"y2","migrated":true,"title":"referenced first"}` {
		t.Fatalf("migration of y2 was reverted while migrating y1: %s", got)
	}

	for run := 0; run < 5; run++ {
		err := api_storage.ExecuteMigrations([]api_storage.MigrationQuery{{Entity: "chain", Action: "set", Properties: map[string]any{"run": float64(run)}}})
		if err != nil {
			t.Fatalf("migration: %v", err)
		}

		want := map[string]string{
			"x1": `{"id":"x1","related":[{"@entity":"chain","id":"x2"}],"run":` + toJSON(t, run) + `,"title":"first"}`,
			"x2": `{"id":"x2","next":{"@entity":"chain","id":"x1"},"run":` + toJSON(t, run) + `,"title":"second"}`,
			"x3": `{"id":"x3","next":{"@entity":"chain","id":"x2"},"related":[{"@entity":"chain","id":"x1"}],"run":` + toJSON(t, run) + `,"title":"third"}`,
		}
		for id, expected := range want {
			if got := toJSON(t, api_storage.ReadEntityById("chain", id)); got != expected {
				t.Fatalf("run %d: %s is %s, want %s", run, id, got, expected)
			}
		}
	}
}

func TestIncludesAllStopsOnCyclesAndKeepsStoredData(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("node", map[string]any{"id": "n1", "next": map[string]any{"@entity": "node", "id": "n2"}, "meta": map[string]any{"owner": map[string]any{"@entity": "node", "id": "n2"}}})
	api_storage.WriteEntity("node", map[string]any{"id": "n2", "next": map[string]any{"@entity": "node", "id": "n1"}})

	done := make(chan []map[string]any, 1)
	go func() { done <- api_storage.ListEntities("node", 0, 0, "", true, nil, "", "all") }()

	select {
	case got := <-done:
		want := `[{"id":"n1","meta":{"owner":{"@entity":"node","id":"n2"}},"next":{"@entity":"node","id":"n2","next":{"@entity":"node","id":"n1","meta":{"owner":{"@entity":"node","id":"n2"}},"next":{"@entity":"node","id":"n2"}}}},` +
			`{"id":"n2","next":{"@entity":"node","id":"n1","meta":{"owner":{"@entity":"node","id":"n2"}},"next":{"@entity":"node","id":"n2","next":{"@entity":"node","id":"n1"}}}}]`
		if toJSON(t, got) != want {
			t.Fatalf("unexpected includes:\n%s\nwant\n%s", toJSON(t, got), want)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("includes=all did not stop on a reference cycle")
	}

	api_storage.ListEntities("node", 0, 0, "", true, nil, "", "meta.owner,next")
	if got := toJSON(t, api_storage.ReadEntityById("node", "n1")); got != `{"id":"n1","meta":{"owner":{"@entity":"node","id":"n2"}},"next":{"@entity":"node","id":"n2"}}` {
		t.Fatalf("includes modified the stored entity: %s", got)
	}
}

func TestSortOrderIsTotalAndMatchesMongoDB(t *testing.T) {
	initTestStore(t)

	docs := []map[string]any{
		{"id": "a", "v": float64(3)},
		{"id": "b"},
		{"id": "c", "v": nil},
		{"id": "d", "v": []any{}},
		{"id": "e", "v": []any{float64(5), float64(1)}},
		{"id": "f", "v": "x"},
		{"id": "g", "v": true},
		{"id": "h", "v": map[string]any{"z": float64(1)}},
		{"id": "i", "v": "2024-01-01T00:00:00Z"},
		{"id": "j", "v": float64(2)},
		{"id": "k", "v": []any{float64(0), float64(9)}},
		{"id": "l", "v": "2024-01-01T00:00:00.0001Z"},
		{"id": "m", "v": false},
	}
	for _, d := range docs {
		api_storage.WriteEntity("sortable", d)
	}

	asc := listIds(api_storage.ListEntities("sortable", 0, 0, "v", true, nil, "", ""))
	if toJSON(t, asc) != `["d","b","c","k","e","j","a","f","h","m","g","i","l"]` {
		t.Fatalf("unexpected ascending order: %v", asc)
	}

	desc := listIds(api_storage.ListEntities("sortable", 0, 0, "v", false, nil, "", ""))
	if toJSON(t, desc) != `["i","l","g","m","h","f","k","e","a","j","b","c","d"]` {
		t.Fatalf("unexpected descending order: %v", desc)
	}
}

func TestUpdateKeepsManualStrictSchema(t *testing.T) {
	initTestStore(t)
	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true

	api_storage.UpdateEntitySchema("book", map[string]any{"title": map[string]any{"type": "string", "required": true}})
	api_storage.WriteEntity("book", map[string]any{"id": "b1", "title": "Dune"})
	api_storage.UpdateEntityById("book", "b1", map[string]any{"title": "Dune Messiah"})

	if !schema.IsManualSchema("book", api_storage.GetEntitySchema("book")) {
		t.Fatalf("update replaced the manual schema: %s", toJSON(t, api_storage.GetEntitySchema("book")))
	}
}

func TestDateFiltersWithMixedOperatorsAreDeterministic(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("event", map[string]any{"id": "e1", "at": "2024-01-15T10:00:00Z"})

	filters := map[string]map[string]string{"at": {"neq": "2024-01-15", "lt": "not a date"}}
	first := len(api_storage.ListEntities("event", 0, 0, "", true, filters, "", ""))
	for i := 0; i < 50; i++ {
		if got := len(api_storage.ListEntities("event", 0, 0, "", true, filters, "", "")); got != first {
			t.Fatalf("filter result changed between calls: %d then %d", first, got)
		}
	}
}

func TestQueryWithNegativeOffsetDoesNotPanic(t *testing.T) {
	initTestStore(t)

	api_storage.WriteEntity("item", map[string]any{"id": "i1"})
	api_storage.WriteEntity("item", map[string]any{"id": "i2"})

	got, err := api_storage.ExecuteQuery(query.Query{Entity: "item", Offset: -5, Limit: 1, Filter: query.FilterNode{Leaf: map[string]map[string]string{}}})
	if err != nil || toJSON(t, listIds(got)) != `["i1"]` {
		t.Fatalf("unexpected result %v (%v)", got, err)
	}
}
