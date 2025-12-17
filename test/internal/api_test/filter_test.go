package api_test

import (
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
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
