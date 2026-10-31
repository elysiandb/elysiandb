package schema_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
)

func setSchemaConfig(strict bool) {
	cfg := &configuration.Config{}
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = strict
	globals.SetConfig(cfg)
}

func fieldOf(t *testing.T, storable map[string]any, path ...string) map[string]any {
	t.Helper()

	current := storable
	for _, name := range path {
		fields, ok := current["fields"].(map[string]any)
		if !ok {
			t.Fatalf("no fields in %v", current)
		}

		field, ok := fields[name].(map[string]any)
		if !ok {
			t.Fatalf("field %s missing in %v", name, fields)
		}

		current = field
	}

	return current
}

func TestMergeAnalyzedSchema_KeepsExistingFieldsAndTypes(t *testing.T) {
	setSchemaConfig(false)

	existing := schema.AnalyzeEntitySchema("inf", map[string]any{"a": "x", "b": 1.0})
	merged := schema.MergeAnalyzedSchema("inf", existing, map[string]any{"a": 5.0, "c": true})

	if got := fieldOf(t, merged, "a")["type"]; got != "string" {
		t.Fatalf("expected the existing type of a to be kept, got %v", got)
	}

	if got := fieldOf(t, merged, "b")["type"]; got != "number" {
		t.Fatalf("expected b to be kept, got %v", got)
	}

	if got := fieldOf(t, merged, "c")["type"]; got != "boolean" {
		t.Fatalf("expected c to be added, got %v", got)
	}

	if merged["id"] != "inf" {
		t.Fatalf("expected the schema id, got %v", merged["id"])
	}

	if _, ok := merged["_manual"]; ok {
		t.Fatalf("an inferred schema must stay inferred")
	}
}

func TestMergeAnalyzedSchema_WithoutExistingSchema(t *testing.T) {
	setSchemaConfig(true)

	merged := schema.MergeAnalyzedSchema("inf", nil, map[string]any{"a": "x"})
	field := fieldOf(t, merged, "a")
	if field["type"] != "string" || field["required"] != true {
		t.Fatalf("expected the analyzed schema, got %v", field)
	}
}

func TestMergeAnalyzedSchema_UpgradesUnknownTypes(t *testing.T) {
	setSchemaConfig(false)

	existing := schema.AnalyzeEntitySchema("nul", map[string]any{"x": nil, "y": "s"})
	merged := schema.MergeAnalyzedSchema("nul", existing, map[string]any{"x": "a", "y": nil})

	if got := fieldOf(t, merged, "x")["type"]; got != "string" {
		t.Fatalf("expected x to take the first concrete type, got %v", got)
	}

	if got := fieldOf(t, merged, "y")["type"]; got != "string" {
		t.Fatalf("expected a null value to keep the type of y, got %v", got)
	}
}

func TestMergeAnalyzedSchema_MergesNestedObjectsAndArrays(t *testing.T) {
	setSchemaConfig(false)

	existing := schema.AnalyzeEntitySchema("n", map[string]any{
		"author": map[string]any{"name": "a"},
		"items":  []any{map[string]any{"sku": "x"}},
	})
	merged := schema.MergeAnalyzedSchema("n", existing, map[string]any{
		"author": map[string]any{"age": 3.0},
		"items":  []any{map[string]any{"qty": 1.0}},
	})

	if fieldOf(t, merged, "author", "name")["type"] != "string" || fieldOf(t, merged, "author", "age")["type"] != "number" {
		t.Fatalf("expected nested object fields to be merged, got %v", merged)
	}

	if fieldOf(t, merged, "items", "sku")["type"] != "string" || fieldOf(t, merged, "items", "qty")["type"] != "number" {
		t.Fatalf("expected array item fields to be merged, got %v", merged)
	}
}

func TestMergeAnalyzedSchema_KeepsManualFlagAndRequiredFlags(t *testing.T) {
	setSchemaConfig(false)

	existing := map[string]any{
		"id":      "m",
		"_manual": true,
		"fields": map[string]any{
			"title": map[string]any{"name": "title", "type": "string", "required": true},
		},
	}

	merged := schema.MergeAnalyzedSchema("m", existing, map[string]any{"title": "x", "extra": 1.0})

	if merged["_manual"] != true {
		t.Fatalf("expected the manual flag to be kept, got %v", merged)
	}

	if fieldOf(t, merged, "title")["required"] != true {
		t.Fatalf("expected the required flag to be kept, got %v", merged)
	}

	if fieldOf(t, merged, "extra")["required"] != false {
		t.Fatalf("expected new fields to follow the strict setting, got %v", merged)
	}
}

func manualSchema(fields map[string]any) map[string]any {
	return map[string]any{"id": "film", "_manual": true, "fields": fields}
}

func TestValidateEntity_NullValues(t *testing.T) {
	setSchemaConfig(true)

	s := manualSchema(map[string]any{
		"title": map[string]any{"type": "string", "required": true},
		"year":  map[string]any{"type": "number", "required": false},
	})

	if errs := schema.ValidateEntity("film", map[string]any{"title": "C", "year": nil}, s); len(errs) != 0 {
		t.Fatalf("expected null to be accepted on an optional field, got %v", errs)
	}

	errs := schema.ValidateEntity("film", map[string]any{"title": nil}, s)
	if len(errs) != 1 || errs[0].Field != "title" || errs[0].Message != schema.MessageRequiredFieldNull {
		t.Fatalf("expected null to be rejected on a required field, got %v", errs)
	}

	setSchemaConfig(false)
	inferred := map[string]any{"id": "film", "fields": map[string]any{
		"title": map[string]any{"type": "string", "required": true},
		"x":     map[string]any{"type": "unknown", "required": false},
	}}

	if errs := schema.ValidateEntity("film", map[string]any{"title": nil, "x": "any"}, inferred); len(errs) != 0 {
		t.Fatalf("expected null and unknown types to be accepted without strict mode, got %v", errs)
	}
}

func TestValidateEntityUpdate_ValidatesTheMergedDocument(t *testing.T) {
	setSchemaConfig(true)

	s := manualSchema(map[string]any{
		"title": map[string]any{"type": "string", "required": true},
		"year":  map[string]any{"type": "number", "required": false},
	})
	existing := map[string]any{"id": "f1", "title": "C", "year": 1983.0}

	if errs := schema.ValidateEntityUpdate("film", existing, map[string]any{"year": 1984.0}, s); len(errs) != 0 {
		t.Fatalf("expected a partial update to pass, got %v", errs)
	}

	if errs := schema.ValidateEntityUpdate("film", existing, map[string]any{"year": "x"}, s); len(errs) != 1 || errs[0].Field != "year" {
		t.Fatalf("expected a type error on year, got %v", errs)
	}

	if errs := schema.ValidateEntityUpdate("film", existing, map[string]any{"extra": 1.0}, s); len(errs) != 1 || errs[0].Field != "extra" {
		t.Fatalf("expected an unknown field to be rejected, got %v", errs)
	}

	if errs := schema.ValidateEntityUpdate("film", map[string]any{"id": "f2", "year": 1.0}, map[string]any{"year": 2.0}, s); len(errs) != 1 || errs[0].Message != schema.MessageRequiredFieldMissing {
		t.Fatalf("expected a missing required field to be reported, got %v", errs)
	}

	legacy := map[string]any{"id": "f3", "title": "C", "legacy": "x", "year": "bad"}
	if errs := schema.ValidateEntityUpdate("film", legacy, map[string]any{"title": "D"}, s); len(errs) != 0 {
		t.Fatalf("expected stored fields the update does not touch to be ignored, got %v", errs)
	}

	if errs := schema.ValidateEntityUpdate("film", nil, map[string]any{"year": 1.0}, s); len(errs) != 1 || errs[0].Field != "title" {
		t.Fatalf("expected the body alone to be validated without a stored document, got %v", errs)
	}
}

func TestMapToFields_ShorthandTypes(t *testing.T) {
	fields := schema.MapToFields(map[string]any{
		"title": "string",
		"pages": map[string]any{"type": "number", "required": true},
	})

	if fields["title"].Type != "string" || fields["title"].Name != "title" || fields["title"].Required {
		t.Fatalf("expected the shorthand form to define a string field, got %+v", fields["title"])
	}

	if fields["pages"].Type != "number" || !fields["pages"].Required {
		t.Fatalf("expected the full form to keep working, got %+v", fields["pages"])
	}
}
