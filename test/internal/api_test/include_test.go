package api_test

import (
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
)

var store = map[string]map[string]interface{}{
	"category:e6a49734": {"id": "e6a49734", "type": "gogo", "@entity": "category"},
	"job:1234567890":    {"id": "1234567890", "designation": "Worker", "@entity": "job"},
	"author:32e1f608": {
		"id":       "32e1f608",
		"fullname": "Taymour Negib",
		"status":   "writer",
		"job": map[string]interface{}{
			"@entity": "job",
			"id":      "1234567890",
		},
		"@entity": "author",
	},
	"author:25667686": {
		"id":       "25667686",
		"fullname": "Alberto",
		"status":   "coco",
		"job": map[string]interface{}{
			"@entity": "job",
			"id":      "1234567890",
		},
		"@entity": "author",
	},
}

func mockReadEntityById(entity string, id string) map[string]interface{} {
	key := entity + ":" + id
	if v, ok := store[key]; ok {
		return v
	}
	if len(id) > 8 {
		key = entity + ":" + id[:8]
		if v, ok := store[key]; ok {
			return v
		}
	}
	return nil
}

func init() {
	api_storage.ReadEntityByIdFunc = mockReadEntityById
}

func normalizeAuthors(raw interface{}) []map[string]interface{} {
	if raw == nil {
		return nil
	}
	switch v := raw.(type) {
	case []interface{}:
		out := make([]map[string]interface{}, 0, len(v))
		for _, el := range v {
			if m, ok := el.(map[string]interface{}); ok {
				out = append(out, m)
			}
		}
		return out
	case []map[string]interface{}:
		return v
	default:
		return nil
	}
}

func TestApplyIncludes_SimpleInclude(t *testing.T) {
	data := []map[string]interface{}{
		{
			"id":   "d5a3e752",
			"name": "youyou",
			"category": map[string]interface{}{
				"@entity": "category",
				"id":      "e6a49734",
			},
		},
	}
	result := api_storage.ApplyIncludes(data, "category")
	category, ok := result[0]["category"].(map[string]interface{})
	if !ok || category["type"] != "gogo" || category["@entity"] != "category" {
		t.Fatalf("unexpected category: %+v", category)
	}
}

func TestApplyIncludes_MultipleIncludes(t *testing.T) {
	data := []map[string]interface{}{
		{
			"id":   "d5a3e752",
			"name": "youyou",
			"category": map[string]interface{}{
				"@entity": "category",
				"id":      "e6a49734",
			},
			"authors": []interface{}{
				map[string]interface{}{
					"@entity": "author",
					"id":      "32e1f608",
				},
				map[string]interface{}{
					"@entity": "author",
					"id":      "25667686",
				},
			},
		},
	}
	result := api_storage.ApplyIncludes(data, "category,authors,authors.job")
	entity := result[0]
	cat := entity["category"].(map[string]interface{})
	if cat["type"] != "gogo" {
		t.Fatalf("category not included properly: %+v", cat)
	}
	authors := normalizeAuthors(entity["authors"])
	if len(authors) != 2 {
		t.Fatalf("unexpected authors length: %d", len(authors))
	}
	for _, a := range authors {
		if a["fullname"] == nil {
			t.Fatalf("author not loaded: %+v", a)
		}
		job, ok := a["job"].(map[string]interface{})
		if !ok || job["designation"] != "Worker" {
			t.Fatalf("job not loaded properly: %+v", job)
		}
	}
}

func TestApplyIncludes_AllMode(t *testing.T) {
	data := []map[string]interface{}{
		{
			"id":   "test1",
			"name": "entity1",
			"category": map[string]interface{}{
				"@entity": "category",
				"id":      "e6a49734",
			},
			"authors": []interface{}{
				map[string]interface{}{
					"@entity": "author",
					"id":      "32e1f608",
				},
			},
		},
	}
	result := api_storage.ApplyIncludes(data, "all")
	entity := result[0]
	cat := entity["category"].(map[string]interface{})
	if cat["type"] != "gogo" {
		t.Fatalf("category not included in all mode: %+v", cat)
	}
	authors := normalizeAuthors(entity["authors"])
	if len(authors) == 0 {
		t.Fatalf("authors not included in all mode: %+v", entity["authors"])
	}
	a := authors[0]
	if a["fullname"] != "Taymour Negib" {
		t.Fatalf("author not included in all mode: %+v", a)
	}
	job := a["job"].(map[string]interface{})
	if job["designation"] != "Worker" {
		t.Fatalf("nested job not included in all mode: %+v", job)
	}
}

func TestApplyIncludes_EmptyInclude(t *testing.T) {
	data := []map[string]interface{}{
		{"id": "1", "name": "simple"},
	}
	result := api_storage.ApplyIncludes(data, "")
	if !reflect.DeepEqual(result, data) {
		t.Fatalf("expected unchanged data, got %+v", result)
	}
}

func TestApplyIncludes_MissingRelations(t *testing.T) {
	data := []map[string]interface{}{
		{
			"id":   "x",
			"name": "test",
			"unknown": map[string]interface{}{
				"@entity": "missing",
				"id":      "000",
			},
		},
	}
	result := api_storage.ApplyIncludes(data, "unknown")
	val := result[0]["unknown"].(map[string]interface{})
	if val["@entity"] != "missing" {
		t.Fatalf("unexpected missing entity behavior: %+v", val)
	}
}
