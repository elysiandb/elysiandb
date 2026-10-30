package api_test

import (
	"reflect"
	"strings"
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

func mockReadEntityById(entity, id string) map[string]interface{} {
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
	result := api_storage.ApplyIncludes(data, "category", mockReadEntityById)
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
	result := api_storage.ApplyIncludes(data, "category,authors,authors.job", mockReadEntityById)
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
	result := api_storage.ApplyIncludes(data, "all", mockReadEntityById)
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
	result := api_storage.ApplyIncludes(data, "", mockReadEntityById)
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
	result := api_storage.ApplyIncludes(data, "unknown", mockReadEntityById)
	val := result[0]["unknown"].(map[string]interface{})
	if val["@entity"] != "missing" {
		t.Fatalf("unexpected missing entity behavior: %+v", val)
	}
}

func guardedStoreReader(entity, id string) map[string]interface{} {
	docs := map[string]map[string]interface{}{
		"_elysiandb_core_user:admin": {"id": "admin", "password": "hash"},
		"author:a1":                  {"id": "a1", "owner": "u", "job": map[string]interface{}{"@entity": "job", "id": "j1"}},
		"author:a2":                  {"id": "a2", "owner": "x"},
		"job:j1":                     {"id": "j1", "designation": "Worker"},
	}

	if v, ok := docs[entity+":"+id]; ok {
		return v
	}

	return nil
}

func guardedPost() []map[string]interface{} {
	return []map[string]interface{}{
		{
			"id":    "p1",
			"admin": map[string]interface{}{"@entity": "_elysiandb_core_user", "id": "admin"},
			"authors": []interface{}{
				map[string]interface{}{"@entity": "author", "id": "a1"},
				map[string]interface{}{"@entity": "author", "id": "a2"},
			},
		},
	}
}

func TestPublicReadGuard_NeverExpandsCoreEntities(t *testing.T) {
	guard := api_storage.PublicReadGuard(func(string, map[string]any) bool { return true })

	for _, includes := range []string{"all", "admin", "admin,authors.job"} {
		result := api_storage.ApplyIncludes(guardedPost(), includes, guard.Apply(guardedStoreReader))

		want := map[string]interface{}{"@entity": "_elysiandb_core_user", "id": "admin"}
		if !reflect.DeepEqual(result[0]["admin"], want) {
			t.Fatalf("includes=%s: expected core reference to stay unexpanded, got %+v", includes, result[0]["admin"])
		}
	}

	result := api_storage.ApplyIncludes(guardedPost(), "all", guard.Apply(guardedStoreReader))
	authors := result[0]["authors"].([]map[string]interface{})
	if authors[0]["job"].(map[string]interface{})["designation"] != "Worker" {
		t.Fatalf("expected non-core references to be expanded, got %+v", authors)
	}
}

func TestPublicReadGuard_DeniedEntitiesStayReferences(t *testing.T) {
	var checked []string
	guard := api_storage.PublicReadGuard(func(entity string, data map[string]any) bool {
		checked = append(checked, entity+":"+data["id"].(string))
		return entity != "job" && data["owner"] != "x"
	})

	result := api_storage.ApplyIncludes(guardedPost(), "authors.job", guard.Apply(guardedStoreReader))
	authors := result[0]["authors"].([]map[string]interface{})

	if authors[0]["owner"] != "u" {
		t.Fatalf("expected readable author to be expanded, got %+v", authors[0])
	}

	wantJob := map[string]interface{}{"@entity": "job", "id": "j1"}
	if !reflect.DeepEqual(authors[0]["job"], wantJob) {
		t.Fatalf("expected denied job to stay a reference, got %+v", authors[0]["job"])
	}

	wantAuthor := map[string]interface{}{"@entity": "author", "id": "a2"}
	if !reflect.DeepEqual(authors[1], wantAuthor) {
		t.Fatalf("expected denied author to stay a reference, got %+v", authors[1])
	}

	if strings.Join(checked, ",") != "author:a1,job:j1,author:a2" {
		t.Fatalf("expected the loaded documents to be checked, got %v", checked)
	}
}

func TestPublicReadGuard_FiltersCannotSeeDeniedReferences(t *testing.T) {
	guard := api_storage.PublicReadGuard(func(string, map[string]any) bool { return true })
	result := api_storage.ApplyIncludes(guardedPost(), api_storage.ExtractAutoIncludes(map[string]map[string]string{
		"admin.password": {"eq": "nope"},
	}), guard.Apply(guardedStoreReader))

	if !api_storage.FiltersMatchEntity(result[0], map[string]map[string]string{"admin.password": {"eq": "nope"}}) {
		t.Fatalf("expected the filter to ignore the unexpanded core reference")
	}
}

func TestReadGuard_NilKeepsReaderUnrestricted(t *testing.T) {
	var guard api_storage.ReadGuard
	result := api_storage.ApplyIncludes(guardedPost(), "all", guard.Apply(guardedStoreReader))

	if result[0]["admin"].(map[string]interface{})["password"] != "hash" {
		t.Fatalf("expected internal includes to keep reading every entity type, got %+v", result[0]["admin"])
	}
}

func TestExtractAutoIncludes_Simple(t *testing.T) {
	in := map[string]map[string]string{
		"author.name": {"eq": "John"},
	}
	got := api_storage.ExtractAutoIncludes(in)
	if got != "author" {
		t.Fatalf("expected author, got %v", got)
	}
}

func TestExtractAutoIncludes_Multiple(t *testing.T) {
	in := map[string]map[string]string{
		"author.name":        {"eq": "John"},
		"author.profile.age": {"eq": "20"},
		"category.title":     {"eq": "news"},
		"title":              {"eq": "x"},
	}
	got := api_storage.ExtractAutoIncludes(in)
	s := map[string]bool{}
	for _, v := range strings.Split(got, ",") {
		s[v] = true
	}
	if !s["author"] || !s["author.profile"] || !s["category"] {
		t.Fatalf("auto includes mismatch: %v", got)
	}
}

func TestMergeIncludes(t *testing.T) {
	a := "author,profile"
	b := "profile,category"
	got := api_storage.MergeIncludes(a, b)
	s := map[string]bool{}
	for _, v := range strings.Split(got, ",") {
		s[v] = true
	}
	if !s["author"] || !s["profile"] || !s["category"] {
		t.Fatalf("merge mismatch: %v", got)
	}
}
