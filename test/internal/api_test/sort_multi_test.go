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

func TestSortEntitiesTiesMissingValuesAndOtherTypes(t *testing.T) {
	data := []map[string]any{
		{"id": "x", "k": 2.0},
		{"id": "y", "k": 1.0},
		{"id": "z", "v": 1.0, "k": 1.0},
		{"id": "same1", "v": 1.0, "k": 1.0},
	}

	api_storage.SortEntities(data, []query.Sort{{Field: "v", Ascending: true}, {Field: "k", Ascending: true}})
	if got := idsOf(data); !reflect.DeepEqual(got, []string{"z", "same1", "y", "x"}) {
		t.Fatalf("expected ties to keep their order and missing values to sort by the next key, got %v", got)
	}

	flags := []map[string]any{
		{"id": "t", "flag": true},
		{"id": "f", "flag": false},
		{"id": "t2", "flag": true},
	}
	api_storage.SortEntities(flags, []query.Sort{{Field: "flag", Ascending: true}, {Field: "id", Ascending: false}})
	if got := idsOf(flags); !reflect.DeepEqual(got, []string{"f", "t2", "t"}) {
		t.Fatalf("expected false before true, got %v", got)
	}

	values := []map[string]any{
		{"id": "obj", "v": map[string]any{"a": 1.0}},
		{"id": "arr", "v": []any{1.0}},
		{"id": "bool", "v": false},
	}
	api_storage.SortEntities(values, []query.Sort{{Field: "v", Ascending: true}, {Field: "id", Ascending: true}})
	if got := idsOf(values); !reflect.DeepEqual(got, []string{"bool", "arr", "obj"}) {
		t.Fatalf("expected objects and arrays after scalar values, got %v", got)
	}
}

func TestSortEntitiesGoNumberTypes(t *testing.T) {
	data := []map[string]any{
		{"id": "i64", "n": int64(5)},
		{"id": "f32", "n": float32(1.5)},
		{"id": "i32", "n": int32(3)},
		{"id": "int", "n": 2},
		{"id": "f64", "n": 4.0},
	}

	api_storage.SortEntities(data, []query.Sort{{Field: "n", Ascending: true}, {Field: "id", Ascending: true}})
	if got := idsOf(data); !reflect.DeepEqual(got, []string{"f32", "int", "i32", "f64", "i64"}) {
		t.Fatalf("expected numeric order across Go number types, got %v", got)
	}
}

func TestPrimarySort(t *testing.T) {
	field, ascending := query.Query{Sorts: map[string]string{"year": "desc", "title": "asc"}, SortOrder: []string{"year", "title"}}.PrimarySort()
	if field != "year" || ascending {
		t.Fatalf("expected year desc, got %s %v", field, ascending)
	}

	field, ascending = query.Query{}.PrimarySort()
	if field != "" || !ascending {
		t.Fatalf("expected no sort, got %s %v", field, ascending)
	}
}
