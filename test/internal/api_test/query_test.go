package api_test

import (
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
)

func TestApplyQueryFilter_EmptyNodeMatchesNothing(t *testing.T) {
	data := []map[string]any{
		{"id": "1", "title": "abc"},
	}
	out := api_storage.ApplyQueryFilter(data, api_storage.FilterNode{})
	if len(out) != 0 {
		t.Fatalf("expected empty result, got %v", out)
	}
}

func TestApplyQueryFilter_PrimitivesAndNestedArrays(t *testing.T) {
	data := []map[string]any{
		{
			"id":     "1",
			"title":  "abcXYZdef",
			"price":  float64(10),
			"flag":   true,
			"tags":   []any{"Go", "kv"},
			"author": map[string]any{"name": "Taymour Negib"},
			"categories": []any{
				map[string]any{"title": "coco"},
				map[string]any{"title": "caca"},
			},
		},
		{
			"id":    "2",
			"title": "nope",
		},
	}

	f := api_storage.FilterNode{
		And: []api_storage.FilterNode{
			{Leaf: map[string]map[string]string{"price": {"gte": "9"}}},
			{Leaf: map[string]map[string]string{"flag": {"eq": "true"}}},
			{Leaf: map[string]map[string]string{"tags": {"contains": "Go"}}},
			{Leaf: map[string]map[string]string{"author.name": {"eq": "Taymour*"}}},
			{Leaf: map[string]map[string]string{"categories.title": {"eq": "coco"}}},
		},
	}

	out := api_storage.ApplyQueryFilter(data, f)
	if len(out) != 1 || out[0]["id"] != "1" {
		t.Fatalf("expected only id=1, got %v", out)
	}
}

func TestApplyQueryFilter_GlobStrictMatchBranches(t *testing.T) {
	data := []map[string]any{
		{"id": "1", "title": "abcXYZdef"},
	}

	cases := []struct {
		name   string
		filter api_storage.FilterNode
		want   int
	}{
		{"star matches all", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"eq": "*"}}}, 1},
		{"exact match no star", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"eq": "abcXYZdef"}}}, 1},
		{"prefix and suffix match", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"eq": "abc*def"}}}, 1},
		{"contains with leading trailing star", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"eq": "*XYZ*"}}}, 1},
		{"multiple stars with empty parts", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"eq": "**XYZ**"}}}, 1},
		{"must end when no trailing star", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"eq": "*XYZ"}}}, 0},
		{"prefix mismatch", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"eq": "zzz*def"}}}, 0},
		{"neq rejects when pattern matches", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"neq": "*XYZ*"}}}, 0},
		{"neq passes when pattern does not match", api_storage.FilterNode{Leaf: map[string]map[string]string{"title": {"neq": "*NOPE*"}}}, 1},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			out := api_storage.ApplyQueryFilter(data, tc.filter)
			if len(out) != tc.want {
				t.Fatalf("expected %d results, got %d (%v)", tc.want, len(out), out)
			}
		})
	}
}

func TestApplyQueryFilter_OrAndNodes(t *testing.T) {
	data := []map[string]any{
		{"id": "1", "title": "Go", "status": "published"},
		{"id": "2", "title": "Python", "status": "draft"},
	}

	f := api_storage.FilterNode{
		Or: []api_storage.FilterNode{
			{Leaf: map[string]map[string]string{"title": {"eq": "Go"}}},
			{Leaf: map[string]map[string]string{"title": {"eq": "Rust"}}},
		},
	}

	out := api_storage.ApplyQueryFilter(data, f)
	if len(out) != 1 || out[0]["id"] != "1" {
		t.Fatalf("expected only id=1, got %v", out)
	}
}

func TestApplyQueryFilter_UnsupportedValueTypeFails(t *testing.T) {
	data := []map[string]any{
		{"id": "1", "meta": map[string]any{"a": 1}},
	}
	filter := api_storage.FilterNode{
		Leaf: map[string]map[string]string{
			"meta": {"eq": "*"},
		},
	}
	out := api_storage.ApplyQueryFilter(data, filter)
	if len(out) != 0 {
		t.Fatalf("expected empty result, got %v", out)
	}
}

func TestExecuteQuery_WithSortOffsetLimit(t *testing.T) {
	initTestStore(t)

	entity := "articles"
	api_storage.WriteEntity(entity, map[string]any{"id": "a1", "status": "published", "title": "Go"})
	api_storage.WriteEntity(entity, map[string]any{"id": "a2", "status": "published", "title": "Rust"})
	api_storage.WriteEntity(entity, map[string]any{"id": "a3", "status": "published", "title": "Python"})

	q := api_storage.Query{
		Entity: entity,
		Offset: 1,
		Limit:  1,
		Sorts:  map[string]string{"title": "asc"},
		Filter: api_storage.FilterNode{
			Leaf: map[string]map[string]string{"status": {"eq": "published"}},
		},
	}

	out, err := api_storage.ExecuteQuery(q)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if len(out) != 1 {
		t.Fatalf("expected 1 result, got %v", out)
	}

	if out[0]["id"] != "a3" {
		t.Fatalf("expected id=a3 after sort+offset, got %v", out[0]["id"])
	}
}

func TestExecuteQuery_ComplexNestedQuery(t *testing.T) {
	initTestStore(t)

	entity := "article"
	doc := map[string]any{
		"id":      "x1",
		"status":  "published",
		"title":   "Pourquoi Go est le langage idéal",
		"excerpt": "Go est adapté aux systèmes distribués",
		"author":  map[string]any{"id": "u789", "name": "Taymour Negib"},
		"tags":    []any{"Go", "distributed"},
		"categories": []any{
			map[string]any{"title": "coco"},
			map[string]any{"title": "caca"},
		},
	}
	api_storage.WriteEntity(entity, doc)

	q := api_storage.Query{
		Entity: entity,
		Filter: api_storage.FilterNode{
			And: []api_storage.FilterNode{
				{Leaf: map[string]map[string]string{"status": {"eq": "published"}}},
				{
					Or: []api_storage.FilterNode{
						{Leaf: map[string]map[string]string{"title": {"eq": "*Go*"}}},
						{Leaf: map[string]map[string]string{"excerpt": {"eq": "*distribuées*"}}},
					},
				},
			},
		},
	}

	out, err := api_storage.ExecuteQuery(q)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(out) != 1 {
		t.Fatalf("expected 1 result, got %v", out)
	}

	got := out[0]
	if got["id"] != "x1" {
		t.Fatalf("expected id x1, got %v", got["id"])
	}

	wantAuthor := map[string]any{"id": "u789", "name": "Taymour Negib"}
	if !reflect.DeepEqual(normalizeMapAny(wantAuthor), normalizeMapAny(got["author"].(map[string]any))) {
		t.Fatalf("author mismatch: got %v", got["author"])
	}
}

func normalizeMapAny(m map[string]any) map[string]any {
	out := make(map[string]any, len(m))
	for k, v := range m {
		switch tv := v.(type) {
		case []any:
			cp := make([]any, len(tv))
			copy(cp, tv)
			out[k] = cp
		case map[string]any:
			out[k] = normalizeMapAny(tv)
		default:
			out[k] = v
		}
	}
	return out
}
