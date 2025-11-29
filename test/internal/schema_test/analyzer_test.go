package schema_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/schema"
)

func patchLoadSchema(mock *schema.Entity) func() {
	orig := schema.LoadSchemaForEntity
	schema.LoadSchemaForEntity = func(entity string) *schema.Entity {
		return mock
	}
	return func() {
		schema.LoadSchemaForEntity = orig
	}
}

func TestAnalyzeEntitySchema_Simple(t *testing.T) {
	data := map[string]interface{}{
		"id":   "u1",
		"name": "Alice",
		"age":  30,
	}

	result := schema.AnalyzeEntitySchema("users", data)

	if result["id"] != "users" {
		t.Fatalf("expected id=users, got %v", result["id"])
	}

	fields := result["fields"].(map[string]interface{})

	if fields["name"].(map[string]interface{})["type"] != "string" {
		t.Fatalf("expected type string for name, got %v", fields["name"].(map[string]interface{})["type"])
	}

	if fields["age"].(map[string]interface{})["type"] != "number" {
		t.Fatalf("expected type number for age, got %v", fields["age"].(map[string]interface{})["type"])
	}
}

func TestAnalyzeEntitySchema_Nested(t *testing.T) {
	data := map[string]interface{}{
		"id":   "p1",
		"name": "Post",
		"author": map[string]interface{}{
			"name": "John",
			"age":  42,
		},
	}

	result := schema.AnalyzeEntitySchema("posts", data)
	fields := result["fields"].(map[string]interface{})
	author := fields["author"].(map[string]interface{})
	subFields := author["fields"].(map[string]interface{})

	if subFields["name"].(map[string]interface{})["type"] != "string" {
		t.Fatalf("expected nested field type string for author.name, got %v", subFields["name"].(map[string]interface{})["type"])
	}
	if subFields["age"].(map[string]interface{})["type"] != "number" {
		t.Fatalf("expected nested field type number for author.age, got %v", subFields["age"].(map[string]interface{})["type"])
	}
}

func TestAnalyzeEntitySchema_ArrayNested(t *testing.T) {
	data := map[string]interface{}{
		"tags": []interface{}{
			map[string]interface{}{
				"label": "go",
				"score": 1,
			},
		},
	}

	result := schema.AnalyzeEntitySchema("items", data)
	fields := result["fields"].(map[string]interface{})
	tags := fields["tags"].(map[string]interface{})
	sub := tags["fields"].(map[string]interface{})

	if sub["label"].(map[string]interface{})["type"] != "string" {
		t.Fatalf("expected string for tags.label")
	}
	if sub["score"].(map[string]interface{})["type"] != "number" {
		t.Fatalf("expected number for tags.score")
	}
}

func TestDetectJSONType_All(t *testing.T) {
	cases := map[string]interface{}{
		"string":  "a",
		"number":  12,
		"boolean": true,
		"object":  map[string]interface{}{"x": 1},
		"array":   []interface{}{1, 2},
	}

	for expected, value := range cases {
		if schema.DetectJSONType(value) != expected {
			t.Fatalf("expected %s, got %s", expected, schema.DetectJSONType(value))
		}
	}
}

func TestValidateEntity_ValidData(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "users",
		Fields: map[string]schema.Field{
			"name": {Name: "name", Type: "string"},
			"age":  {Name: "age", Type: "number"},
		},
	}
	restore := patchLoadSchema(mockSchema)
	defer restore()

	data := map[string]interface{}{
		"name": "Alice",
		"age":  30,
	}

	errors := schema.ValidateEntity("users", data)
	if len(errors) != 0 {
		t.Fatalf("expected no validation errors, got %v", errors)
	}
}

func TestValidateEntity_TypeMismatch(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "users",
		Fields: map[string]schema.Field{
			"name": {Name: "name", Type: "string"},
			"age":  {Name: "age", Type: "number"},
		},
	}
	restore := patchLoadSchema(mockSchema)
	defer restore()

	data := map[string]interface{}{
		"name": 123,
		"age":  "old",
	}

	errors := schema.ValidateEntity("users", data)
	if len(errors) != 2 {
		t.Fatalf("expected 2 validation errors, got %d", len(errors))
	}
}

func TestValidateEntity_NestedMismatch(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "orders",
		Fields: map[string]schema.Field{
			"customer": {
				Name: "customer",
				Type: "object",
				Fields: map[string]schema.Field{
					"name": {Name: "name", Type: "string"},
					"age":  {Name: "age", Type: "number"},
				},
			},
		},
	}
	restore := patchLoadSchema(mockSchema)
	defer restore()

	data := map[string]interface{}{
		"customer": map[string]interface{}{
			"name": 123,
			"age":  "old",
		},
	}

	errors := schema.ValidateEntity("orders", data)
	if len(errors) != 2 {
		t.Fatalf("expected 2 nested validation errors, got %d", len(errors))
	}
}

func TestValidateEntity_ArrayMismatch(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "tags",
		Fields: map[string]schema.Field{
			"tags": {
				Name: "tags",
				Type: "array",
				Fields: map[string]schema.Field{
					"label": {Name: "label", Type: "string"},
				},
			},
		},
	}
	restore := patchLoadSchema(mockSchema)
	defer restore()

	data := map[string]interface{}{
		"tags": "notArray",
	}

	errors := schema.ValidateEntity("tags", data)
	if len(errors) != 1 {
		t.Fatalf("expected 1 error, got %d", len(errors))
	}
}

func TestMapToFields_RoundTrip(t *testing.T) {
	fields := map[string]schema.Field{
		"a": {Name: "a", Type: "string"},
		"b": {Name: "b", Type: "number", Fields: map[string]schema.Field{
			"c": {Name: "c", Type: "boolean"},
		}},
	}
	mapped := schema.FieldsToMap(fields)
	round := schema.MapToFields(mapped)

	if round["a"].Type != "string" {
		t.Fatalf("roundtrip failed for field a")
	}
	if round["b"].Fields["c"].Type != "boolean" {
		t.Fatalf("roundtrip failed for nested c")
	}
}

func TestSchemaEntityToStorableStructure(t *testing.T) {
	ent := schema.Entity{
		ID: "test",
		Fields: map[string]schema.Field{
			"x": {Name: "x", Type: "string"},
		},
	}

	out := schema.SchemaEntityToStorableStructure(ent)
	if out["id"] != "test" {
		t.Fatalf("id mismatch")
	}
	fields := out["fields"].(map[string]interface{})
	if fields["x"].(map[string]interface{})["type"] != "string" {
		t.Fatalf("field missing")
	}
}

func TestIsManualSchema_NoFlag(t *testing.T) {
	if schema.IsManualSchema("unknown") {
		t.Fatalf("expected false for unknown manual schema")
	}
}
