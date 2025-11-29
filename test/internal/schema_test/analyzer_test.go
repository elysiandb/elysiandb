package schema_test

import (
	"reflect"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/storage"
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

func patchGetJsonByKey(m map[string]interface{}, err error) func() {
	orig := storage.GetJsonByKey
	storage.GetJsonByKey = func(key string) (map[string]interface{}, error) {
		return m, err
	}
	return func() {
		storage.GetJsonByKey = orig
	}
}

func patchManualFlag(val bool) func() {
	if val {
		return patchGetJsonByKey(map[string]interface{}{"_manual": true}, nil)
	}
	return patchGetJsonByKey(map[string]interface{}{}, nil)
}

func patchStrict(val bool) func() {
	cfg := globals.GetConfig()
	if cfg == nil {
		cfg = &configuration.Config{}
	}
	orig := cfg.Api.Schema.Strict
	cfg.Api.Schema.Strict = val
	globals.SetConfig(cfg)

	return func() {
		cfg2 := globals.GetConfig()
		if cfg2 == nil {
			return
		}
		cfg2.Api.Schema.Strict = orig
		globals.SetConfig(cfg2)
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
		t.Fatalf("expected id=users")
	}

	fields := result["fields"].(map[string]interface{})

	if fields["name"].(map[string]interface{})["type"] != "string" {
		t.Fatalf("expected string")
	}
	if fields["age"].(map[string]interface{})["type"] != "number" {
		t.Fatalf("expected number")
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
	sub := author["fields"].(map[string]interface{})

	if sub["name"].(map[string]interface{})["type"] != "string" {
		t.Fatalf("expected string")
	}
	if sub["age"].(map[string]interface{})["type"] != "number" {
		t.Fatalf("expected number")
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
		t.Fatalf("expected string")
	}
	if sub["score"].(map[string]interface{})["type"] != "number" {
		t.Fatalf("expected number")
	}
}

func TestDetectJSONType(t *testing.T) {
	tests := []struct {
		in  interface{}
		exp string
	}{
		{"x", "string"},
		{30, "number"},
		{true, "boolean"},
		{map[string]interface{}{"a": 1}, "object"},
		{[]interface{}{1, 2}, "array"},
		{nil, "unknown"},
	}

	for _, tt := range tests {
		if got := schema.DetectJSONType(tt.in); got != tt.exp {
			t.Fatalf("expected %s got %s", tt.exp, got)
		}
	}
}

func TestValidateEntity_Strict_NewFieldRejected(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "users",
		Fields: map[string]schema.Field{
			"name": {Name: "name", Type: "string"},
		},
	}

	restore1 := patchLoadSchema(mockSchema)
	defer restore1()

	restore2 := patchManualFlag(true)
	defer restore2()

	restore3 := patchStrict(true)
	defer restore3()

	data := map[string]interface{}{
		"name": "Alice",
		"age":  30,
	}

	errs := schema.ValidateEntity("users", data)
	if len(errs) != 1 {
		t.Fatalf("expected 1 error")
	}
}

func TestValidateEntity_Strict_DeepNewFieldRejected(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "books",
		Fields: map[string]schema.Field{
			"author": {
				Name: "author",
				Type: "object",
				Fields: map[string]schema.Field{
					"name": {Name: "name", Type: "string"},
					"age":  {Name: "age", Type: "number"},
				},
			},
		},
	}

	restore1 := patchLoadSchema(mockSchema)
	defer restore1()

	restore2 := patchManualFlag(true)
	defer restore2()

	restore3 := patchStrict(true)
	defer restore3()

	data := map[string]interface{}{
		"author": map[string]interface{}{
			"name":  "Alice",
			"age":   40,
			"extra": "no",
			"nested": map[string]interface{}{
				"x": 1,
			},
		},
	}

	errs := schema.ValidateEntity("books", data)
	if len(errs) != 2 {
		t.Fatalf("expected 2 errors")
	}
}

func TestValidateEntity_TypeMismatch(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "u",
		Fields: map[string]schema.Field{
			"age": {Name: "age", Type: "number"},
		},
	}

	restore := patchLoadSchema(mockSchema)
	defer restore()

	data := map[string]interface{}{"age": "wrong"}

	errs := schema.ValidateEntity("u", data)
	if len(errs) != 1 {
		t.Fatal("expected 1 error")
	}
}

func TestValidateEntity_NoSchema(t *testing.T) {
	restore := patchLoadSchema(nil)
	defer restore()

	errs := schema.ValidateEntity("x", map[string]interface{}{"a": 1})
	if len(errs) != 0 {
		t.Fatal("expected no errors")
	}
}

func TestIsManualSchema_NoFlag(t *testing.T) {
	restore := patchManualFlag(false)
	defer restore()

	if schema.IsManualSchema("unknown") {
		t.Fatalf("expected false")
	}
}

func TestIsManualSchema_YesFlag(t *testing.T) {
	restore := patchManualFlag(true)
	defer restore()

	if !schema.IsManualSchema("e") {
		t.Fatalf("expected true")
	}
}

func TestFieldsToMapAndBack(t *testing.T) {
	fields := map[string]schema.Field{
		"name": {
			Name: "name",
			Type: "string",
			Fields: map[string]schema.Field{
				"sub": {Name: "sub", Type: "number"},
			},
		},
	}

	m := schema.FieldsToMap(fields)
	rev := schema.MapToFields(m)

	if !reflect.DeepEqual(fields["name"].Type, rev["name"].Type) {
		t.Fatalf("mismatch")
	}
	if rev["name"].Fields["sub"].Type != "number" {
		t.Fatalf("missing nested field")
	}
}

func TestSchemaEntityToStorableStructure(t *testing.T) {
	e := schema.Entity{ID: "x", Fields: map[string]schema.Field{"a": {Name: "a", Type: "string"}}}
	out := schema.SchemaEntityToStorableStructure(e)
	if out["id"] != "x" {
		t.Fatal("wrong id")
	}
}
