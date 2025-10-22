package api_test

import (
	"reflect"
	"testing"

	"github.com/google/uuid"
	api_storage "github.com/taymour/elysiandb/internal/api"
)

func TestExtractSubEntitiesSimple(t *testing.T) {
	data := map[string]interface{}{
		"name": "youyou",
		"author": map[string]interface{}{
			"@entity":  "author",
			"fullname": "Taymour",
		},
	}
	subs := api_storage.ExtractSubEntities("article", data)
	if len(subs) != 1 {
		t.Fatalf("expected 1 sub entity, got %d", len(subs))
	}
	if subs[0]["@entity"] != "author" {
		t.Fatalf("expected sub entity 'author', got %v", subs[0]["@entity"])
	}
	if _, ok := subs[0]["id"].(string); !ok {
		t.Fatalf("expected id field in sub entity")
	}
	a := data["author"].(map[string]interface{})
	if a["@entity"] != "author" {
		t.Fatalf("expected author link to have @entity")
	}
	if _, ok := a["id"].(string); !ok {
		t.Fatalf("expected author link to have id")
	}
}

func TestExtractSubEntitiesNested(t *testing.T) {
	data := map[string]interface{}{
		"name": "youyou",
		"author": map[string]interface{}{
			"@entity":  "author",
			"fullname": "Taymour Negib",
			"status":   "writer",
			"job": map[string]interface{}{
				"@entity":     "job",
				"designation": "Worker",
			},
		},
	}
	subs := api_storage.ExtractSubEntities("article", data)
	if len(subs) != 2 {
		t.Fatalf("expected 2 sub entities, got %d", len(subs))
	}
	var author map[string]interface{}
	var job map[string]interface{}
	for _, s := range subs {
		if s["@entity"] == "author" {
			author = s
		}
		if s["@entity"] == "job" {
			job = s
		}
	}
	if author == nil || job == nil {
		t.Fatalf("missing author or job in subs")
	}
	if job["designation"] != "Worker" {
		t.Fatalf("expected job.designation=Worker, got %v", job["designation"])
	}
	if reflect.TypeOf(author["job"]).Kind() != reflect.Map {
		t.Fatalf("expected author.job to be a map")
	}
	j := author["job"].(map[string]interface{})
	if j["@entity"] != "job" {
		t.Fatalf("expected author.job.@entity=job, got %v", j["@entity"])
	}
	if _, err := uuid.Parse(j["id"].(string)); err != nil {
		t.Fatalf("expected valid uuid for job.id")
	}
}

func TestExtractSubEntitiesArray(t *testing.T) {
	data := map[string]interface{}{
		"name":  "youyou",
		"age":   13,
		"title": "this is it",
		"authors": []interface{}{
			map[string]interface{}{
				"@entity":  "author",
				"fullname": "Taymour Negib",
				"status":   "writer",
				"job": map[string]interface{}{
					"@entity":     "job",
					"designation": "Worker",
				},
			},
			map[string]interface{}{
				"@entity":  "author",
				"fullname": "Alberto",
				"status":   "coco",
			},
		},
	}
	subs := api_storage.ExtractSubEntities("article", data)
	if len(subs) != 3 {
		t.Fatalf("expected 3 sub entities (2 authors + 1 job), got %d", len(subs))
	}
	var authors []map[string]interface{}
	var job map[string]interface{}
	for _, s := range subs {
		if s["@entity"] == "author" {
			authors = append(authors, s)
		}
		if s["@entity"] == "job" {
			job = s
		}
	}
	if len(authors) != 2 {
		t.Fatalf("expected 2 authors, got %d", len(authors))
	}
	if job == nil {
		t.Fatalf("missing job in subs")
	}
	if job["designation"] != "Worker" {
		t.Fatalf("expected job.designation=Worker, got %v", job["designation"])
	}
	arr := data["authors"].([]interface{})
	if len(arr) != 2 {
		t.Fatalf("expected 2 author links, got %d", len(arr))
	}
	for _, a := range arr {
		m := a.(map[string]interface{})
		if m["@entity"] != "author" {
			t.Fatalf("expected @entity=author, got %v", m["@entity"])
		}
		if _, err := uuid.Parse(m["id"].(string)); err != nil {
			t.Fatalf("expected valid uuid for author.id")
		}
	}
}
