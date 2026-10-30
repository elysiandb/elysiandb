package api_test

import (
	"errors"
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

func subEntityPayload() map[string]any {
	return map[string]any{
		"title": "t",
		"author": map[string]any{
			"@entity": "author",
			"id":      "au1",
			"name":    "Ada",
			"job":     map[string]any{"@entity": "job", "@id": "j1", "title": "Engineer"},
			"boss":    map[string]any{"@entity": "author", "id": "au0"},
		},
		"reviewers": []any{
			map[string]any{"@entity": "author", "id": "au2", "name": "Linus"},
			map[string]any{"@entity": "author", "id": "au3"},
			"plain",
		},
		"typed": []map[string]any{{"@entity": "tag", "id": "t1", "label": "go"}},
		"meta":  map[string]any{"source": map[string]any{"@entity": "source", "id": "s1", "url": "u"}},
	}
}

func TestVisitSubEntityWritesMatchesExtractSubEntities(t *testing.T) {
	visited := map[string]bool{}
	err := api_storage.VisitSubEntityWrites(subEntityPayload(), func(entity, id string, sub map[string]any) error {
		visited[entity+"/"+id] = true
		return nil
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	extracted := map[string]bool{}
	for _, sub := range api_storage.ExtractSubEntities("article", subEntityPayload()) {
		extracted[sub["@entity"].(string)+"/"+sub["id"].(string)] = true
	}

	want := map[string]bool{"author/au1": true, "job/j1": true, "author/au2": true, "tag/t1": true, "source/s1": true}
	if !reflect.DeepEqual(visited, want) || !reflect.DeepEqual(extracted, want) {
		t.Fatalf("expected %v, visited %v, extracted %v", want, visited, extracted)
	}
}

func TestVisitSubEntityWritesStopsOnError(t *testing.T) {
	boom := errors.New("boom")
	err := api_storage.VisitSubEntityWrites(subEntityPayload(), func(entity, id string, sub map[string]any) error {
		if entity == "job" {
			return boom
		}
		return nil
	})
	if !errors.Is(err, boom) {
		t.Fatalf("expected boom, got %v", err)
	}
}

func TestRejectCoreSubEntities(t *testing.T) {
	core := map[string]any{"@entity": "_elysiandb_core_user", "id": "admin", "role": "admin"}
	rejected := []map[string]any{
		{"x": core},
		{"x": []any{core}},
		{"x": []map[string]any{core}},
		{"x": map[string]any{"deep": core}},
		{"x": map[string]any{"@entity": "author", "id": "a1", "name": "A", "owner": core}},
		{"x": map[string]any{"@entity": "_elysiandb_core_hook", "name": "no id"}},
	}
	for _, data := range rejected {
		if err := api_storage.RejectCoreSubEntities(data); !errors.Is(err, api_storage.ErrCoreSubEntity) {
			t.Fatalf("expected ErrCoreSubEntity for %v, got %v", data, err)
		}
	}

	allowed := []map[string]any{
		{"x": map[string]any{"@entity": "_elysiandb_core_user", "id": "admin"}},
		{"x": []any{map[string]any{"@entity": "_elysiandb_core_user", "@id": "admin"}}},
		{"x": map[string]any{"@entity": "author", "id": "a1", "owner": map[string]any{"@entity": "_elysiandb_core_user", "id": "admin"}}},
		{"_elysiandb_core_user": map[string]any{"role": "admin"}},
		subEntityPayload(),
	}
	for _, data := range allowed {
		if err := api_storage.RejectCoreSubEntities(data); err != nil {
			t.Fatalf("expected %v to be allowed, got %v", data, err)
		}
	}
}

func TestReferenceCoreSubEntities(t *testing.T) {
	expanded := func() map[string]any {
		return map[string]any{"@entity": "_elysiandb_core_user", "id": "admin", "role": "admin"}
	}
	author := map[string]any{"@entity": "author", "id": "a1", "name": "A", "owner": expanded()}
	data := map[string]any{
		"id":     "p1",
		"owner":  expanded(),
		"list":   []any{expanded(), "x"},
		"typed":  []map[string]any{expanded()},
		"author": author,
		"ref":    map[string]any{"@entity": "_elysiandb_core_user", "id": "bob"},
	}

	api_storage.ReferenceCoreSubEntities(data)

	ref := map[string]any{"@entity": "_elysiandb_core_user", "id": "admin"}
	want := map[string]any{
		"id":     "p1",
		"owner":  ref,
		"list":   []any{ref, "x"},
		"typed":  []map[string]any{ref},
		"author": map[string]any{"@entity": "author", "id": "a1", "name": "A", "owner": ref},
		"ref":    map[string]any{"@entity": "_elysiandb_core_user", "id": "bob"},
	}
	if !reflect.DeepEqual(data, want) {
		t.Fatalf("expected %v, got %v", want, data)
	}
}
