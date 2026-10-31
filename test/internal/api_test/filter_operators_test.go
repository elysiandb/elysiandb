package api_test

import (
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/query"
)

func matches(entity map[string]any, field, op, value string) bool {
	return api_storage.FiltersMatchEntity(entity, map[string]map[string]string{field: {op: value}})
}

func TestFiltersMatchEntityStringComparisons(t *testing.T) {
	entity := map[string]any{"label": "banana"}

	cases := []struct {
		op    string
		value string
		want  bool
	}{
		{"lt", "b", false},
		{"lt", "c", true},
		{"lte", "banana", true},
		{"gt", "apple", true},
		{"gt", "zzz", false},
		{"gte", "banana", true},
		{"contains", "nan", true},
		{"contains", "NAN", true},
		{"contains", "kiwi", false},
		{"not_contains", "nan", false},
		{"not_contains", "kiwi", true},
		{"eq", "ban*", true},
		{"neq", "ban*", false},
		{"any", "banana", false},
		{"all", "banana", false},
		{"unknown", "banana", false},
	}

	for _, c := range cases {
		if got := matches(entity, "label", c.op, c.value); got != c.want {
			t.Errorf("label %s %q: expected %v, got %v", c.op, c.value, c.want, got)
		}
	}
}

func TestFiltersMatchEntityDatesStillCompareAsDates(t *testing.T) {
	entity := map[string]any{"day": "2024-05-01"}

	if !matches(entity, "day", "lt", "2024-06-01") || matches(entity, "day", "gt", "2024-06-01") {
		t.Fatalf("expected date comparisons")
	}

	if !matches(entity, "day", "contains", "-05-") {
		t.Fatalf("expected contains to apply to the date string")
	}

	if !matches(entity, "day", "lt", "notadate") {
		t.Fatalf("expected a comparison with a non-date value to be lexicographic")
	}

	both := map[string]map[string]string{"day": {"gte": "2024-01-01", "lt": "notadate"}}
	if !api_storage.FiltersMatchEntity(entity, both) {
		t.Fatalf("expected mixed date and non-date values to compare as strings")
	}
}

func TestFiltersMatchEntityArrayContainsDoesNotSplitValues(t *testing.T) {
	entity := map[string]any{"tags": []any{"Smith, John", "x"}}

	if !matches(entity, "tags", "contains", "Smith, John") {
		t.Fatalf("expected the whole value to be looked up")
	}

	if matches(entity, "tags", "contains", "Smith") {
		t.Fatalf("expected no partial match on array items")
	}

	if matches(entity, "tags", "contains", "x,zzz") {
		t.Fatalf("expected the value not to be split")
	}

	if !matches(entity, "tags", "not_contains", "x,zzz") || matches(entity, "tags", "not_contains", "x") {
		t.Fatalf("unexpected not_contains result")
	}

	if !matches(entity, "tags", "any", "x,zzz") || matches(entity, "tags", "all", "x,zzz") || !matches(entity, "tags", "none", "a,b") {
		t.Fatalf("expected any, all and none to keep splitting on commas")
	}

	if matches(entity, "tags", "lt", "z") {
		t.Fatalf("expected an unsupported array operator not to match")
	}
}

func TestFiltersMatchEntityUnsupportedOperatorsNeverMatch(t *testing.T) {
	entity := map[string]any{"n": 3.0, "ok": true}

	for _, op := range []string{"contains", "any", "unknown"} {
		if matches(entity, "n", op, "3") {
			t.Errorf("number %s: expected no match", op)
		}

		if matches(entity, "ok", op, "true") {
			t.Errorf("boolean %s: expected no match", op)
		}
	}

	if !matches(entity, "n", "gte", "3") || !matches(entity, "ok", "eq", "true") || !matches(entity, "ok", "neq", "false") {
		t.Fatalf("expected supported operators to keep working")
	}
}

func TestApplyQueryFilterUsesTheSameOperators(t *testing.T) {
	data := []map[string]any{
		{"id": "t1", "label": "banana"},
		{"id": "t2", "label": "apple"},
	}

	out := api_storage.ApplyQueryFilter(data, query.FilterNode{Leaf: map[string]map[string]string{"label": {"lt": "b"}}})
	if len(out) != 1 || out[0]["id"] != "t2" {
		t.Fatalf("expected only apple, got %v", out)
	}
}

func TestSearchPattern(t *testing.T) {
	cases := map[string]string{
		"Zebra":   "*Zebra*",
		"*Zebra*": "*Zebra*",
		"Zeb*":    "Zeb*",
		"":        "**",
	}

	for in, want := range cases {
		if got := api_storage.SearchPattern(in); got != want {
			t.Errorf("SearchPattern(%q): expected %q, got %q", in, want, got)
		}
	}

	if !api_storage.SearchMatchesEntity(map[string]any{"title": "Zebra crossing"}, "zebra") {
		t.Fatalf("expected a plain search to match a substring")
	}

	if !api_storage.SearchMatchesEntity(map[string]any{"title": "Zebra crossing"}, "Zeb") {
		t.Fatalf("expected a prefix to match without a glob")
	}

	if api_storage.SearchMatchesEntity(map[string]any{"title": "Zebra crossing"}, "crossing*") {
		t.Fatalf("expected an explicit glob to keep its meaning")
	}
}

func TestListEntitiesSearchAndFiltersBeforePagination(t *testing.T) {
	initTestStore(t)

	for _, doc := range []map[string]any{
		{"id": "n1", "title": "Note 1", "kind": "k1"},
		{"id": "n2", "title": "Note 2", "kind": "k0"},
		{"id": "n3", "title": "Note 3", "kind": "k1"},
		{"id": "n4", "title": "Note 4", "kind": "k0"},
		{"id": "n5", "title": "Zebra crossing", "kind": "k1"},
	} {
		api_storage.WriteEntity("note", doc)
	}

	ids := func(list []map[string]any) []string {
		var out []string
		for _, item := range list {
			out = append(out, item["id"].(string))
		}
		return out
	}

	if got := ids(api_storage.ListEntities("note", 2, 0, "", true, nil, "Zebra", "")); !reflect.DeepEqual(got, []string{"n5"}) {
		t.Fatalf("expected the match beyond the first page, got %v", got)
	}

	filters := map[string]map[string]string{"kind": {"eq": "k0"}}
	if got := ids(api_storage.ListEntities("note", 0, 0, "id", true, filters, "Note", "")); !reflect.DeepEqual(got, []string{"n2", "n4"}) {
		t.Fatalf("expected search and filters to combine, got %v", got)
	}

	if got := ids(api_storage.ListEntities("note", 1, 1, "id", true, filters, "Note", "")); !reflect.DeepEqual(got, []string{"n4"}) {
		t.Fatalf("expected pagination after matching, got %v", got)
	}

	if got := api_storage.ListEntities("note", 2, 0, "", true, nil, "", ""); len(got) != 2 {
		t.Fatalf("expected plain pagination to keep working, got %v", got)
	}
}
