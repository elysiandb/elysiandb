package api_test

import (
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/query"
)

func idsOf(list []map[string]any) []string {
	out := make([]string, 0, len(list))
	for _, item := range list {
		out = append(out, item["id"].(string))
	}

	return out
}

func TestSortEntitiesUsesEveryKeyInOrder(t *testing.T) {
	data := []map[string]any{
		{"id": "f1", "title": "C", "year": 1983.0},
		{"id": "f2", "title": "B", "year": 1982.0},
		{"id": "f3", "title": "A", "year": 1979.0},
		{"id": "f4", "title": "A", "year": 1983.0},
	}

	api_storage.SortEntities(data, []query.Sort{{Field: "year", Ascending: false}, {Field: "title", Ascending: true}})
	if got := idsOf(data); !reflect.DeepEqual(got, []string{"f4", "f1", "f2", "f3"}) {
		t.Fatalf("year desc, title asc: got %v", got)
	}

	api_storage.SortEntities(data, []query.Sort{{Field: "title", Ascending: true}, {Field: "year", Ascending: true}})
	if got := idsOf(data); !reflect.DeepEqual(got, []string{"f3", "f4", "f2", "f1"}) {
		t.Fatalf("title asc, year asc: got %v", got)
	}
}

func TestSortEntitiesDatesNestedFieldsAndMissingValues(t *testing.T) {
	data := []map[string]any{
		{"id": "a", "meta": map[string]any{"at": "2024-05-01"}, "rank": 1.0},
		{"id": "b", "rank": 1.0},
		{"id": "c", "meta": map[string]any{"at": "2023-01-01T10:00:00Z"}, "rank": 1.0},
		{"id": "d", "meta": map[string]any{"at": "2024-05-01"}, "rank": 0.0},
	}

	api_storage.SortEntities(data, []query.Sort{{Field: "meta.at", Ascending: true}, {Field: "rank", Ascending: true}})
	if got := idsOf(data); !reflect.DeepEqual(got, []string{"c", "d", "a", "b"}) {
		t.Fatalf("asc with missing values last: got %v", got)
	}

	api_storage.SortEntities(data, []query.Sort{{Field: "meta.at", Ascending: false}, {Field: "rank", Ascending: false}})
	if got := idsOf(data); !reflect.DeepEqual(got, []string{"a", "d", "c", "b"}) {
		t.Fatalf("desc with missing values last: got %v", got)
	}
}

func TestSortEntitiesMixedTypesAreDeterministic(t *testing.T) {
	build := func() []map[string]any {
		return []map[string]any{
			{"id": "s", "v": "x", "k": 1.0},
			{"id": "b", "v": true, "k": 1.0},
			{"id": "n", "v": 2.0, "k": 1.0},
			{"id": "d", "v": "2024-01-01", "k": 1.0},
		}
	}

	first := build()
	api_storage.SortEntities(first, []query.Sort{{Field: "v", Ascending: true}, {Field: "k", Ascending: true}})
	if got := idsOf(first); !reflect.DeepEqual(got, []string{"n", "d", "s", "b"}) {
		t.Fatalf("expected numbers, dates, strings then booleans, got %v", got)
	}

	for i := 0; i < 20; i++ {
		again := build()
		api_storage.SortEntities(again, []query.Sort{{Field: "v", Ascending: true}, {Field: "k", Ascending: true}})
		if !reflect.DeepEqual(idsOf(again), idsOf(first)) {
			t.Fatalf("expected a stable order, got %v then %v", idsOf(first), idsOf(again))
		}
	}
}

func TestExecuteQueryWithSeveralSorts(t *testing.T) {
	initTestStore(t)

	for _, doc := range []map[string]any{
		{"id": "f1", "title": "C", "year": 1983.0},
		{"id": "f2", "title": "B", "year": 1982.0},
		{"id": "f3", "title": "A", "year": 1979.0},
		{"id": "f4", "title": "A", "year": 1983.0},
	} {
		api_storage.WriteEntity("flm", doc)
	}

	for i := 0; i < 10; i++ {
		out, err := api_storage.ExecuteQuery(query.Query{
			Entity:    "flm",
			Filter:    query.FilterNode{Leaf: map[string]map[string]string{}},
			Sorts:     map[string]string{"year": "desc", "title": "asc"},
			SortOrder: []string{"year", "title"},
			Offset:    1,
			Limit:     2,
		})
		if err != nil {
			t.Fatal(err)
		}

		if got := idsOf(out); !reflect.DeepEqual(got, []string{"f1", "f2"}) {
			t.Fatalf("expected [f1 f2], got %v", got)
		}
	}
}

func TestOrderedSorts(t *testing.T) {
	q := query.Query{
		Sorts:     map[string]string{"year": "DESC", "title": "asc", "rank": "whatever"},
		SortOrder: []string{"year", "missing", "title", "year"},
	}

	want := []query.Sort{
		{Field: "year", Ascending: false},
		{Field: "title", Ascending: true},
		{Field: "rank", Ascending: true},
	}

	if got := q.OrderedSorts(); !reflect.DeepEqual(got, want) {
		t.Fatalf("expected %v, got %v", want, got)
	}

	unordered := query.Query{Sorts: map[string]string{"b": "asc", "a": "desc"}}
	if got := unordered.OrderedSorts(); !reflect.DeepEqual(got, []query.Sort{{Field: "a", Ascending: false}, {Field: "b", Ascending: true}}) {
		t.Fatalf("expected alphabetical order without an explicit order, got %v", got)
	}

	if got := (query.Query{}).OrderedSorts(); len(got) != 0 {
		t.Fatalf("expected no sort, got %v", got)
	}
}
