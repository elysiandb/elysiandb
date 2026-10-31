package api_test

import (
	"reflect"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/query"
)

func TestSearchMatchesEntitySimple(t *testing.T) {
	entity := map[string]interface{}{
		"name": "Alice",
	}
	if !api_storage.SearchMatchesEntity(entity, "Ali*") {
		t.Fatalf("expected simple glob match")
	}
	if api_storage.SearchMatchesEntity(entity, "Bob*") {
		t.Fatalf("expected simple glob fail")
	}
}

func TestSearchMatchesEntityNestedMap(t *testing.T) {
	entity := map[string]interface{}{
		"user": map[string]interface{}{
			"profile": map[string]interface{}{
				"city": "Paris",
			},
		},
	}
	if !api_storage.SearchMatchesEntity(entity, "Par*") {
		t.Fatalf("expected nested match")
	}
	if api_storage.SearchMatchesEntity(entity, "Lon*") {
		t.Fatalf("expected nested fail")
	}
}

func TestSearchMatchesEntityNestedArray(t *testing.T) {
	entity := map[string]interface{}{
		"tags": []interface{}{"blue", "green", "red"},
	}
	if !api_storage.SearchMatchesEntity(entity, "gr*") {
		t.Fatalf("expected array match")
	}
	if api_storage.SearchMatchesEntity(entity, "ye*") {
		t.Fatalf("expected no match")
	}
}

func TestSearchMatchesEntityMixedStructures(t *testing.T) {
	entity := map[string]interface{}{
		"data": []interface{}{
			map[string]interface{}{
				"info": []interface{}{
					"hello",
					map[string]interface{}{
						"deep": "world",
					},
				},
			},
		},
	}
	if !api_storage.SearchMatchesEntity(entity, "wor*") {
		t.Fatalf("expected deep match")
	}
	if api_storage.SearchMatchesEntity(entity, "xxx") {
		t.Fatalf("expected deep fail")
	}
}

func TestSearchMatchesEntityNoStringValues(t *testing.T) {
	entity := map[string]interface{}{
		"nums": []interface{}{1, 2, 3},
		"obj": map[string]interface{}{
			"flag": false,
		},
	}
	if api_storage.SearchMatchesEntity(entity, "*") {
		t.Fatalf("expected no match with no string values")
	}
}

func TestSearchMatchesEntitySkipsIdsMarkersAndCoreFields(t *testing.T) {
	entity := map[string]any{
		"id":                       "m1",
		"_elysiandb_core_username": "admin",
		"title":                    "Blade Runner",
		"directors": []any{
			map[string]any{"@entity": "director", "id": "ridley-scott"},
		},
	}

	for _, search := range []string{"m1", "admin", "director", "scott", "*admin*"} {
		if api_storage.SearchMatchesEntity(entity, search) {
			t.Fatalf("expected %q not to match ids, markers or core fields", search)
		}
	}

	if !api_storage.SearchMatchesEntity(entity, "runner") {
		t.Fatalf("expected content fields to stay searchable")
	}
}

func TestSearchMatchesEntityIncludedDocumentContent(t *testing.T) {
	entity := map[string]any{
		"id":    "f1",
		"title": "Blade Runner",
		"directors": []any{
			map[string]any{
				"@entity":                  "director",
				"id":                       "ridley-scott",
				"_elysiandb_core_username": "admin",
				"name":                     "Ridley Scott",
			},
		},
	}

	if !api_storage.SearchMatchesEntity(entity, "scott") {
		t.Fatalf("expected the content of an included document to be searchable")
	}

	for _, search := range []string{"ridley-scott", "admin", "director"} {
		if api_storage.SearchMatchesEntity(entity, search) {
			t.Fatalf("expected %q not to match the id, owner or marker of an included document", search)
		}
	}
}

func TestSearchMatchesEntityIncludedList(t *testing.T) {
	entity := map[string]any{
		"title": "Blade Runner",
		"directors": []map[string]any{
			{"@entity": "director", "id": "ridley-scott", "name": "Ridley Scott"},
		},
	}

	if !api_storage.SearchMatchesEntity(entity, "scott") {
		t.Fatalf("expected the content of an included list to be searchable")
	}

	if api_storage.SearchMatchesEntity(entity, "ridley-scott") {
		t.Fatalf("expected the ids of an included list to be skipped")
	}
}

func TestGetNestedValue(t *testing.T) {
	data := map[string]interface{}{
		"user": map[string]interface{}{
			"name": "Alice",
			"info": map[string]interface{}{
				"age":  30.0,
				"date": "2025-09-27T10:00:00Z",
			},
		},
	}
	val, ok := api_storage.GetNestedValue(data, "user.name")
	if !ok || val.(string) != "Alice" {
		t.Fatalf("expected Alice, got %v", val)
	}
	val, ok = api_storage.GetNestedValue(data, "user.info.age")
	if !ok || val.(float64) != 30.0 {
		t.Fatalf("expected 30, got %v", val)
	}
	val, ok = api_storage.GetNestedValue(data, "user.info.date")
	if !ok || val.(string) != "2025-09-27T10:00:00Z" {
		t.Fatalf("expected date string, got %v", val)
	}
	_, ok = api_storage.GetNestedValue(data, "user.unknown")
	if ok {
		t.Fatalf("expected false for missing field")
	}
	_, ok = api_storage.GetNestedValue(data, "user.info.age.year")
	if ok {
		t.Fatalf("expected false for invalid deep path")
	}
}

func TestFiltersMatchEntityStringGlob(t *testing.T) {
	entity := map[string]interface{}{"name": "Alice"}
	filters := map[string]map[string]string{
		"name": {"eq": "Ali*"},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected match for glob")
	}
	filters = map[string]map[string]string{
		"name": {"neq": "Alice"},
	}
	if api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected not match for neq")
	}
}

func TestFiltersMatchEntityFloatOps(t *testing.T) {
	entity := map[string]interface{}{"age": 30.0}
	filters := map[string]map[string]string{
		"age": {"eq": "30"},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected eq match")
	}
	filters = map[string]map[string]string{
		"age": {"neq": "30"},
	}
	if api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected neq fail")
	}
	filters = map[string]map[string]string{
		"age": {"lt": "40"},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected lt match")
	}
	filters = map[string]map[string]string{
		"age": {"lte": "30"},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected lte match")
	}
	filters = map[string]map[string]string{
		"age": {"gt": "20"},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected gt match")
	}
	filters = map[string]map[string]string{
		"age": {"gte": "30"},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected gte match")
	}
}

func TestFiltersMatchEntityDateOps(t *testing.T) {
	now := time.Now().UTC()
	entity := map[string]interface{}{"createdAt": now.Format(time.RFC3339)}
	before := now.Add(-time.Hour).Format(time.RFC3339)
	after := now.Add(time.Hour).Format(time.RFC3339)

	filters := map[string]map[string]string{
		"createdAt": {"eq": now.Format(time.RFC3339)},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected eq date match")
	}

	filters = map[string]map[string]string{
		"createdAt": {"neq": now.Format(time.RFC3339)},
	}
	if api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected neq fail on same date")
	}

	filters = map[string]map[string]string{
		"createdAt": {"lt": after},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected lt match")
	}

	filters = map[string]map[string]string{
		"createdAt": {"lte": now.Format(time.RFC3339)},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected lte match")
	}

	filters = map[string]map[string]string{
		"createdAt": {"gt": before},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected gt match")
	}

	filters = map[string]map[string]string{
		"createdAt": {"gte": now.Format(time.RFC3339)},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected gte match")
	}
}

func TestFiltersMatchEntityDateOnlyAgainstDateTime(t *testing.T) {
	entity := map[string]interface{}{"createdAt": "2023-05-10T15:04:05Z"}

	f := map[string]map[string]string{"createdAt": {"eq": "2023-05-10"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected eq match with date-only vs datetime")
	}

	f = map[string]map[string]string{"createdAt": {"neq": "2023-05-10"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected neq fail with same date-only")
	}

	f = map[string]map[string]string{"createdAt": {"lt": "2023-05-11"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected lt match with date-only > datetime date")
	}

	f = map[string]map[string]string{"createdAt": {"gt": "2023-05-09"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected gt match with date-only < datetime date")
	}

	f = map[string]map[string]string{"createdAt": {"lte": "2023-05-10"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected lte match on same date-only")
	}

	f = map[string]map[string]string{"createdAt": {"gte": "2023-05-10"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected gte match on same date-only")
	}
}

func TestFiltersMatchEntityDateOnlyOps(t *testing.T) {
	today := time.Date(2023, 5, 10, 0, 0, 0, 0, time.UTC)
	entity := map[string]interface{}{"createdAt": today.Format("2006-01-02")}
	before := today.AddDate(0, 0, -1).Format("2006-01-02")
	after := today.AddDate(0, 0, 1).Format("2006-01-02")

	filters := map[string]map[string]string{
		"createdAt": {"eq": today.Format("2006-01-02")},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected eq date-only match")
	}

	filters = map[string]map[string]string{
		"createdAt": {"neq": today.Format("2006-01-02")},
	}
	if api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected neq fail on same date-only")
	}

	filters = map[string]map[string]string{
		"createdAt": {"lt": after},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected lt date-only match")
	}

	filters = map[string]map[string]string{
		"createdAt": {"lte": today.Format("2006-01-02")},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected lte date-only match")
	}

	filters = map[string]map[string]string{
		"createdAt": {"gt": before},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected gt date-only match")
	}

	filters = map[string]map[string]string{
		"createdAt": {"gte": today.Format("2006-01-02")},
	}
	if !api_storage.FiltersMatchEntity(entity, filters) {
		t.Fatalf("expected gte date-only match")
	}
}

func TestFiltersMatchEntityArrayContains(t *testing.T) {
	entity := map[string]interface{}{"tags": []interface{}{"toto", "tata", "titi"}}
	f := map[string]map[string]string{"tags": {"contains": "toto"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected contains match")
	}
	f = map[string]map[string]string{"tags": {"contains": "tutu"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected contains fail")
	}
}

func TestFiltersMatchEntityArrayNotContains(t *testing.T) {
	entity := map[string]interface{}{"tags": []interface{}{"toto", "tata", "titi"}}
	f := map[string]map[string]string{"tags": {"not_contains": "tutu"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected not_contains match")
	}
	f = map[string]map[string]string{"tags": {"not_contains": "toto"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected not_contains fail")
	}
}

func TestFiltersMatchEntityArrayAll(t *testing.T) {
	entity := map[string]interface{}{"tags": []interface{}{"toto", "tata", "titi"}}
	f := map[string]map[string]string{"tags": {"all": "toto,tata"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected all match")
	}
	f = map[string]map[string]string{"tags": {"all": "toto,tutu"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected all fail")
	}
}

func TestFiltersMatchEntityArrayAny(t *testing.T) {
	entity := map[string]interface{}{"tags": []interface{}{"toto", "tata", "titi"}}
	f := map[string]map[string]string{"tags": {"any": "tutu,tata"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected any match")
	}
	f = map[string]map[string]string{"tags": {"any": "tutu,tete"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected any fail")
	}
}

func TestFiltersMatchEntityArrayEq(t *testing.T) {
	entity := map[string]interface{}{"tags": []interface{}{"toto", "tata", "titi"}}
	f := map[string]map[string]string{"tags": {"eq": "toto,tata,titi"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected eq match")
	}
	f = map[string]map[string]string{"tags": {"eq": "toto,tata"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected eq fail length mismatch")
	}
	f = map[string]map[string]string{"tags": {"eq": "toto,titi,tata"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected eq match ignoring order")
	}
}

func TestFiltersMatchEntityArrayNone(t *testing.T) {
	entity := map[string]interface{}{"tags": []interface{}{"toto", "tata", "titi"}}
	f := map[string]map[string]string{"tags": {"none": "tutu,tete"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected none match")
	}
	f = map[string]map[string]string{"tags": {"none": "tata,tete"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected none fail")
	}
}

func TestFiltersMatchEntityArrayMixedTypes(t *testing.T) {
	entity := map[string]interface{}{"values": []interface{}{"1", 2.0, "three"}}
	f := map[string]map[string]string{"values": {"contains": "1"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected contains match for stringified int")
	}
	f = map[string]map[string]string{"values": {"contains": "2"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected contains match for float")
	}
	f = map[string]map[string]string{"values": {"any": "two,three"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected any match for string value")
	}
	f = map[string]map[string]string{"values": {"none": "four,five"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected none match for absent values")
	}
}

func TestFiltersMatchEntityMissingFieldDoesNotMatch(t *testing.T) {
	entity := map[string]interface{}{"id": "nf", "title": "No year"}
	for _, op := range []string{"eq", "neq", "lt", "lte", "gt", "gte", "contains", "not_contains", "any", "all", "none"} {
		f := map[string]map[string]string{"year": {op: "1982"}}
		if api_storage.FiltersMatchEntity(entity, f) {
			t.Fatalf("expected %s on missing field to fail", op)
		}
	}

	f := map[string]map[string]string{"author.name": {"eq": "Alice"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected filter on missing nested field to fail")
	}

	f = map[string]map[string]string{"title": {"eq": "No year"}, "year": {"gte": "1982"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected missing field to fail even when other filters match")
	}
}

func TestFiltersMatchEntityNullValueDoesNotMatch(t *testing.T) {
	entity := map[string]interface{}{"year": nil}
	f := map[string]map[string]string{"year": {"gte": "1982"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected filter on null field to fail")
	}
}

func TestFiltersMatchEntityArrayTraversal(t *testing.T) {
	entity := map[string]interface{}{
		"cats": []interface{}{
			map[string]interface{}{"name": "sf", "meta": map[string]interface{}{"rank": 1.0}},
			map[string]interface{}{"name": "noir", "meta": map[string]interface{}{"rank": 4.0}},
		},
	}

	f := map[string]map[string]string{"cats.name": {"eq": "sf"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected match when an array element satisfies the filter")
	}

	f = map[string]map[string]string{"cats.name": {"eq": "comedy"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected no match when no array element satisfies the filter")
	}

	f = map[string]map[string]string{"cats.meta.rank": {"gte": "3"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected match on nested path through array")
	}

	f = map[string]map[string]string{"cats.meta.rank": {"gt": "4"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected no match on nested path through array")
	}
}

func TestFiltersMatchEntityTraversesIncludedRelationList(t *testing.T) {
	entity := map[string]interface{}{
		"authors": []map[string]interface{}{
			{"name": "Alice", "job": map[string]interface{}{"title": "Writer"}},
			{"name": "Bob"},
		},
	}

	f := map[string]map[string]string{"authors.name": {"eq": "Bob"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected match on included relation list")
	}

	f = map[string]map[string]string{"authors.job.title": {"eq": "Writer"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected match on nested field of included relation list")
	}

	f = map[string]map[string]string{"authors.name": {"eq": "Carol"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected no match on included relation list")
	}
}

func TestFiltersMatchEntityIntegerValues(t *testing.T) {
	entity := map[string]interface{}{"score": int64(5), "rank": 3, "level": int32(2)}

	f := map[string]map[string]string{"score": {"gte": "5"}, "rank": {"eq": "3"}, "level": {"lt": "3"}}
	if !api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected integer values to match")
	}

	f = map[string]map[string]string{"score": {"gt": "5"}}
	if api_storage.FiltersMatchEntity(entity, f) {
		t.Fatalf("expected integer value comparison to fail")
	}
}

func TestFiltersMatchEntitySameSemanticsAsQueryFilter(t *testing.T) {
	data := []map[string]any{
		{"id": "f1", "year": 1981.0},
		{"id": "f2", "year": 1982.0},
		{"id": "nf", "title": "No year"},
		{"id": "t1", "cats": []any{map[string]any{"name": "sf"}, map[string]any{"name": "noir"}}},
		{"id": "t2", "cats": []any{map[string]any{"name": "comedy"}}},
		{"id": "n1", "year": nil, "cats": "sf"},
	}

	filters := []map[string]map[string]string{
		{"year": {"gte": "1982"}},
		{"year": {"neq": "1982"}},
		{"cats.name": {"eq": "sf"}},
		{"cats.name": {"neq": "sf"}},
		{"cats": {"eq": "sf"}},
		{"title": {"eq": "*"}},
	}

	for _, f := range filters {
		want := api_storage.ApplyQueryFilter(data, query.FilterNode{Leaf: f})
		got := api_storage.ApplyFiltersToList(data, f)
		if !reflect.DeepEqual(want, got) {
			t.Fatalf("filter %v: REST %v, query %v", f, got, want)
		}
	}
}

func TestStoredFiltersMatchEntityIgnoresUnresolvedFields(t *testing.T) {
	entity := map[string]interface{}{"tag": "a", "cats": []interface{}{map[string]interface{}{"name": "sf"}}}

	f := map[string]map[string]string{"allowed": {"eq": "true"}, "tag": {"eq": "a"}, "cats.name": {"eq": "sf"}}
	if !api_storage.StoredFiltersMatchEntity(entity, f) {
		t.Fatalf("expected unresolved field to be ignored")
	}

	f = map[string]map[string]string{"allowed": {"eq": "true"}, "tag": {"eq": "b"}}
	if api_storage.StoredFiltersMatchEntity(entity, f) {
		t.Fatalf("expected stored field mismatch to fail")
	}

	f = map[string]map[string]string{"cats.name": {"eq": "noir"}}
	if api_storage.StoredFiltersMatchEntity(entity, f) {
		t.Fatalf("expected array traversal mismatch to fail")
	}
}
