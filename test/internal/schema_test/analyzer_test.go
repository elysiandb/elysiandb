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

	fields, ok := result["fields"].(map[string]interface{})
	if !ok {
		t.Fatalf("expected fields map, got %T", result["fields"])
	}

	if fields["name"].(map[string]interface{})["type"] != "string" {
		t.Fatalf("expected type string for name, got %v", fields["name"].(map[string]interface{})["type"])
	}
	if fields["age"].(map[string]interface{})["type"] != "int" {
		t.Fatalf("expected type int for age, got %v", fields["age"].(map[string]interface{})["type"])
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
	if subFields["age"].(map[string]interface{})["type"] != "int" {
		t.Fatalf("expected nested field type int for author.age, got %v", subFields["age"].(map[string]interface{})["type"])
	}
}

func TestValidateEntity_ValidData(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "users",
		Fields: map[string]schema.Field{
			"name": {Name: "name", Type: "string"},
			"age":  {Name: "age", Type: "int"},
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
			"age":  {Name: "age", Type: "int"},
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
				Type: "map[string]interface {}",
				Fields: map[string]schema.Field{
					"name": {Name: "name", Type: "string"},
					"age":  {Name: "age", Type: "int"},
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
