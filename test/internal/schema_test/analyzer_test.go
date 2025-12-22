package schema_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/storage"
)

func patchLoadSchema(mock *schema.Entity) func() {
	orig := schema.LoadSchemaForEntity
	schema.LoadSchemaForEntity = func(entity string, schemaData map[string]any) *schema.Entity { return mock }
	return func() { schema.LoadSchemaForEntity = orig }
}

func patchGetJsonByKey(m map[string]interface{}, err error) func() {
	orig := storage.GetJsonByKey
	storage.GetJsonByKey = func(key string) (map[string]interface{}, error) { return m, err }
	return func() { storage.GetJsonByKey = orig }
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
		if cfg2 != nil {
			cfg2.Api.Schema.Strict = orig
			globals.SetConfig(cfg2)
		}
	}
}

func TestAnalyzeEntitySchema_Simple(t *testing.T) {
	restore := patchStrict(false)
	defer restore()

	data := map[string]interface{}{
		"id":   "u1",
		"name": "Alice",
		"age":  30,
	}

	result := schema.AnalyzeEntitySchema("users", data)
	fields := result["fields"].(map[string]interface{})

	if fields["name"].(map[string]interface{})["required"] != false {
		t.Fatalf("expected required=false")
	}
	if fields["age"].(map[string]interface{})["required"] != false {
		t.Fatalf("expected required=false")
	}
}

func TestAnalyzeEntitySchema_StrictRequiredTrue(t *testing.T) {
	restore := patchStrict(true)
	defer restore()

	data := map[string]interface{}{
		"name": "Alice",
	}

	result := schema.AnalyzeEntitySchema("users", data)
	fields := result["fields"].(map[string]interface{})

	if fields["name"].(map[string]interface{})["required"] != true {
		t.Fatalf("expected required=true")
	}
}

func TestAnalyzeEntitySchema_Nested(t *testing.T) {
	restore := patchStrict(false)
	defer restore()

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

	if sub["name"].(map[string]interface{})["required"] != false {
		t.Fatalf("expected required=false")
	}
}

func TestAnalyzeEntitySchema_ArrayNested(t *testing.T) {
	restore := patchStrict(false)
	defer restore()

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

	if sub["label"].(map[string]interface{})["required"] != false {
		t.Fatalf("expected required=false")
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
			"name": {Name: "name", Type: "string", Required: true},
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

	errs := schema.ValidateEntity("users", data, nil)
	if len(errs) != 1 {
		t.Fatalf("expected 1 error")
	}
}

func TestValidateEntity_Strict_RequiredMissing(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "users",
		Fields: map[string]schema.Field{
			"name": {Name: "name", Type: "string", Required: true},
			"age":  {Name: "age", Type: "number", Required: true},
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
	}

	errs := schema.ValidateEntity("users", data, nil)
	if len(errs) != 1 {
		t.Fatalf("expected 1 error missing required=age")
	}
}

func TestValidateEntity_Strict_DeepNewFieldRejected(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "books",
		Fields: map[string]schema.Field{
			"author": {
				Name:     "author",
				Type:     "object",
				Required: true,
				Fields: map[string]schema.Field{
					"name": {Name: "name", Type: "string", Required: true},
					"age":  {Name: "age", Type: "number", Required: true},
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

	errs := schema.ValidateEntity("books", data, nil)
	if len(errs) != 2 {
		t.Fatalf("expected 2 errors")
	}
}

func TestValidateEntity_TypeMismatch(t *testing.T) {
	mockSchema := &schema.Entity{
		ID: "u",
		Fields: map[string]schema.Field{
			"age": {Name: "age", Type: "number", Required: false},
		},
	}

	restore := patchLoadSchema(mockSchema)
	defer restore()

	data := map[string]interface{}{"age": "wrong"}

	errs := schema.ValidateEntity("u", data, nil)
	if len(errs) != 1 {
		t.Fatal("expected 1 error")
	}
}

func TestValidateEntity_NoSchema(t *testing.T) {
	restore := patchLoadSchema(nil)
	defer restore()

	errs := schema.ValidateEntity("x", map[string]interface{}{"a": 1}, nil)
	if len(errs) != 0 {
		t.Fatal("expected no errors")
	}
}

func TestIsManualSchema_NoFlag(t *testing.T) {
	restore := patchManualFlag(false)
	defer restore()

	if schema.IsManualSchema("unknown", nil) {
		t.Fatalf("expected false")
	}
}

func TestIsManualSchema_YesFlag(t *testing.T) {
	restore := patchManualFlag(true)
	defer restore()

	if !schema.IsManualSchema("e", nil) {
		t.Fatalf("expected true")
	}
}

func TestFieldsToMapAndBack(t *testing.T) {
	fields := map[string]schema.Field{
		"name": {
			Name:     "name",
			Type:     "string",
			Required: true,
			Fields: map[string]schema.Field{
				"sub": {Name: "sub", Type: "number", Required: false},
			},
		},
	}

	m := schema.FieldsToMap(fields)
	rev := schema.MapToFields(m)

	if rev["name"].Required != true {
		t.Fatalf("expected required=true")
	}
	if rev["name"].Fields["sub"].Required != false {
		t.Fatalf("expected required=false")
	}
}

func TestSchemaEntityToStorableStructure(t *testing.T) {
	e := schema.Entity{
		ID: "x",
		Fields: map[string]schema.Field{
			"a": {Name: "a", Type: "string", Required: true},
		},
	}

	out := schema.SchemaEntityToStorableStructure(e)
	if out["id"] != "x" {
		t.Fatal("wrong id")
	}

	f := out["fields"].(map[string]interface{})
	a := f["a"].(map[string]interface{})
	if a["required"] != true {
		t.Fatal("required not exported")
	}
}

func TestAnalyzeEntitySchema_IgnoresID(t *testing.T) {
	restore := patchStrict(false)
	defer restore()

	data := map[string]interface{}{
		"id":   "abc",
		"name": "hello",
	}

	result := schema.AnalyzeEntitySchema("test", data)
	fields := result["fields"].(map[string]interface{})

	if _, ok := fields["id"]; ok {
		t.Fatalf("id field should not be included")
	}
	if _, ok := fields["name"]; !ok {
		t.Fatalf("expected name field")
	}
}

func TestAnalyzeEntitySchema_ArrayNoSubfields(t *testing.T) {
	restore := patchStrict(false)
	defer restore()

	data := map[string]interface{}{
		"items": []interface{}{},
	}

	result := schema.AnalyzeEntitySchema("arr", data)
	fields := result["fields"].(map[string]interface{})

	item := fields["items"].(map[string]interface{})
	if item["type"] != "array" {
		t.Fatalf("expected array")
	}
	if _, ok := item["fields"]; ok {
		t.Fatalf("expected no subfields for empty array")
	}
}

func TestValidateEntity_ArrayObjectValidation(t *testing.T) {
	mock := &schema.Entity{
		ID: "e",
		Fields: map[string]schema.Field{
			"items": {
				Name:     "items",
				Type:     "array",
				Required: true,
				Fields: map[string]schema.Field{
					"label": {Name: "label", Type: "string", Required: true},
				},
			},
		},
	}

	restore1 := patchLoadSchema(mock)
	defer restore1()

	data := map[string]interface{}{
		"items": []interface{}{
			map[string]interface{}{"label": "ok"},
			map[string]interface{}{"label": 123},
		},
	}

	errs := schema.ValidateEntity("e", data, nil)
	if len(errs) != 1 {
		t.Fatalf("expected 1 error")
	}
}

func TestValidateEntity_ArrayMissingRequired(t *testing.T) {
	mock := &schema.Entity{
		ID: "test",
		Fields: map[string]schema.Field{
			"arr": {
				Name:     "arr",
				Type:     "array",
				Required: true,
				Fields: map[string]schema.Field{
					"n": {Name: "n", Type: "number", Required: true},
				},
			},
		},
	}

	restore1 := patchLoadSchema(mock)
	defer restore1()
	restore2 := patchStrict(true)
	defer restore2()
	restore3 := patchManualFlag(true)
	defer restore3()

	data := map[string]interface{}{
		"arr": []interface{}{
			map[string]interface{}{},
		},
	}

	errs := schema.ValidateEntity("test", data, nil)
	if len(errs) != 1 {
		t.Fatalf("expected missing required error")
	}
}

func TestValidateEntity_ObjectWrongType(t *testing.T) {
	mock := &schema.Entity{
		ID: "wrong",
		Fields: map[string]schema.Field{
			"meta": {Name: "meta", Type: "object", Required: false},
		},
	}

	restore := patchLoadSchema(mock)
	defer restore()

	data := map[string]interface{}{
		"meta": "not-an-object",
	}

	errs := schema.ValidateEntity("wrong", data, nil)
	if len(errs) != 1 {
		t.Fatalf("expected type mismatch error")
	}
}

func TestValidateEntity_ArrayWrongType(t *testing.T) {
	mock := &schema.Entity{
		ID: "wrong",
		Fields: map[string]schema.Field{
			"items": {Name: "items", Type: "array", Required: false},
		},
	}

	restore := patchLoadSchema(mock)
	defer restore()

	data := map[string]interface{}{
		"items": "not-array",
	}

	errs := schema.ValidateEntity("wrong", data, nil)
	if len(errs) != 1 {
		t.Fatalf("expected array mismatch error")
	}
}

func TestMapToFields_NameOverride(t *testing.T) {
	m := map[string]interface{}{
		"x": map[string]interface{}{
			"name":     "real",
			"type":     "string",
			"required": true,
		},
	}

	fields := schema.MapToFields(m)

	if _, ok := fields["real"]; !ok {
		t.Fatalf("expected key 'real'")
	}
}

func TestLoadSchemaForEntity_NoData(t *testing.T) {
	restore := patchGetJsonByKey(nil, nil)
	defer restore()

	res := schema.LoadSchemaForEntity("x", nil)
	if res == nil {
		return
	}
	t.Fatalf("expected nil schema")
}

func TestLoadSchemaForEntity_WithData(t *testing.T) {
	restore := patchGetJsonByKey(map[string]any{
		"fields": map[string]any{
			"a": map[string]any{
				"name":     "a",
				"type":     "string",
				"required": true,
			},
		},
	}, nil)
	defer restore()

	res := schema.LoadSchemaForEntity("x", nil)
	if res == nil {
		t.Fatalf("expected schema")
	}
	if _, ok := res.Fields["a"]; !ok {
		t.Fatalf("expected field a")
	}
}
