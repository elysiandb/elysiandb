package e2e

import (
	"encoding/json"
	"testing"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func TestSchemaStrict_PutSchema_Create_Update_Validation(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true
	globals.SetConfig(cfg)

	put1 := mustPUTJSON(t, client, "http://test/api/books/schema", map[string]any{
		"fields": map[string]any{
			"title": map[string]any{"type": "string", "required": true},
			"pages": map[string]any{"type": "number", "required": true},
		},
	})
	if put1.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 for PUT schema, got %d", put1.StatusCode())
	}

	gr := mustGET(t, client, "http://test/api/books/schema")
	if gr.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 for schema get, got %d", gr.StatusCode())
	}
	var schema map[string]any
	_ = json.Unmarshal(gr.Body(), &schema)
	fields := schema["fields"].(map[string]any)
	if fields["title"].(map[string]any)["type"] != "string" {
		t.Fatalf("title type mismatch")
	}

	c1 := mustPOSTJSON(t, client, "http://test/api/books", map[string]any{
		"title": "Go",
		"pages": 123,
	})
	if c1.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on valid create, got %d", c1.StatusCode())
	}

	c2 := mustPOSTJSON(t, client, "http://test/api/books", map[string]any{
		"title": 999,
		"pages": 123,
	})
	if c2.StatusCode() == fasthttp.StatusOK {
		t.Fatalf("expected validation error for wrong title type")
	}

	var created map[string]any
	_ = json.Unmarshal(c1.Body(), &created)
	id := created["id"].(string)

	up1 := mustPUTJSON(t, client, "http://test/api/books/"+id, map[string]any{
		"title": "Updated Title",
		"pages": 200,
	})
	if up1.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on valid update, got %d", up1.StatusCode())
	}

	up2 := mustPUTJSON(t, client, "http://test/api/books/"+id, map[string]any{
		"title": "Bad",
		"pages": "wrong",
	})
	if up2.StatusCode() == fasthttp.StatusOK {
		t.Fatalf("expected validation error on update wrong type")
	}

	mustPOSTJSON(t, client, "http://test/api/books", map[string]any{
		"title": "New",
		"pages": 10,
	})

	gr2 := mustGET(t, client, "http://test/api/books/schema")
	var schema2 map[string]any
	_ = json.Unmarshal(gr2.Body(), &schema2)
	fields2 := schema2["fields"].(map[string]any)
	if len(fields2) != 2 {
		t.Fatalf("schema should not auto-update in strict mode")
	}
}

func TestSchemaStrict_PutSchema_AddNewFieldRejected(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true
	globals.SetConfig(cfg)

	mustPUTJSON(t, client, "http://test/api/users/schema", map[string]any{
		"fields": map[string]any{
			"name": map[string]any{"type": "string", "required": true},
		},
	})

	r1 := mustPOSTJSON(t, client, "http://test/api/users", map[string]any{
		"name": "Alice",
	})
	if r1.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", r1.StatusCode())
	}

	r2 := mustPOSTJSON(t, client, "http://test/api/users", map[string]any{
		"name": "Bob",
		"age":  22,
	})
	if r2.StatusCode() == fasthttp.StatusOK {
		t.Fatalf("unexpected success: strict schema should reject new field")
	}
}

func TestSchemaStrict_UpdateSchema_ThenValidateNewRules(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true
	globals.SetConfig(cfg)

	mustPUTJSON(t, client, "http://test/api/profiles/schema", map[string]any{
		"fields": map[string]any{
			"username": map[string]any{"type": "string", "required": true},
		},
	})

	r1 := mustPOSTJSON(t, client, "http://test/api/profiles", map[string]any{
		"username": "x",
	})
	if r1.StatusCode() != 200 {
		t.Fatalf("expected 200")
	}

	put2 := mustPUTJSON(t, client, "http://test/api/profiles/schema", map[string]any{
		"fields": map[string]any{
			"username": map[string]any{"type": "string", "required": true},
			"score":    map[string]any{"type": "number", "required": false},
		},
	})
	if put2.StatusCode() != 200 {
		t.Fatalf("expected 200 on schema update")
	}

	r2 := mustPOSTJSON(t, client, "http://test/api/profiles", map[string]any{
		"username": "y",
	})
	if r2.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 without score")
	}

	r3 := mustPOSTJSON(t, client, "http://test/api/profiles", map[string]any{
		"username": "z",
		"score":    10,
	})
	if r3.StatusCode() != 200 {
		t.Fatalf("expected 200 with valid score")
	}

	r4 := mustPOSTJSON(t, client, "http://test/api/profiles", map[string]any{
		"username": "bad",
		"score":    "wrong",
	})
	if r4.StatusCode() == 200 {
		t.Fatalf("expected failure on wrong score type")
	}
}

func TestSchemaStrict_GetAfterPut(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true
	globals.SetConfig(cfg)

	put := mustPUTJSON(t, client, "http://test/api/accounts/schema", map[string]any{
		"fields": map[string]any{
			"email": map[string]any{"type": "string", "required": true},
			"age":   map[string]any{"type": "number", "required": false},
		},
	})
	if put.StatusCode() != 200 {
		t.Fatalf("expected 200")
	}

	gr := mustGET(t, client, "http://test/api/accounts/schema")
	if gr.StatusCode() != 200 {
		t.Fatalf("expected 200")
	}
	var s map[string]any
	_ = json.Unmarshal(gr.Body(), &s)
	f := s["fields"].(map[string]any)
	if f["email"].(map[string]any)["type"] != "string" {
		t.Fatalf("missing email")
	}
	if f["age"].(map[string]any)["type"] != "number" {
		t.Fatalf("missing age")
	}
}

func TestSchemaStrict_DeepNestedStructureValidation(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true
	globals.SetConfig(cfg)

	mustPUTJSON(t, client, "http://test/api/articles/schema", map[string]any{
		"fields": map[string]any{
			"title": map[string]any{"type": "string", "required": true},
			"author": map[string]any{
				"type":     "object",
				"required": true,
				"fields": map[string]any{
					"name": map[string]any{"type": "string", "required": true},
					"meta": map[string]any{
						"type":     "object",
						"required": false,
						"fields": map[string]any{
							"age": map[string]any{"type": "number", "required": false},
						},
					},
				},
			},
		},
	})

	r1 := mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "ok",
		"author": map[string]any{
			"name": "x",
			"meta": map[string]any{
				"age": 22,
			},
		},
	})
	if r1.StatusCode() != 200 {
		t.Fatalf("expected success")
	}

	r2 := mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "bad",
		"author": map[string]any{
			"name": "x",
			"meta": map[string]any{
				"age": "wrong",
			},
		},
	})
	if r2.StatusCode() == 200 {
		t.Fatalf("expected failure")
	}
}

func TestSchemaStrict_ArrayNestedValidation(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true
	globals.SetConfig(cfg)

	mustPUTJSON(t, client, "http://test/api/library/schema", map[string]any{
		"fields": map[string]any{
			"books": map[string]any{
				"type":     "array",
				"required": true,
				"fields": map[string]any{
					"title": map[string]any{"type": "string", "required": true},
					"year":  map[string]any{"type": "number", "required": true},
				},
			},
		},
	})

	r1 := mustPOSTJSON(t, client, "http://test/api/library", map[string]any{
		"books": []any{
			map[string]any{"title": "A", "year": 2000},
			map[string]any{"title": "B", "year": 1999},
		},
	})
	if r1.StatusCode() != 200 {
		t.Fatalf("expected 200")
	}

	r2 := mustPOSTJSON(t, client, "http://test/api/library", map[string]any{
		"books": []any{
			map[string]any{"title": "A", "year": "wrong"},
		},
	})
	if r2.StatusCode() == 200 {
		t.Fatalf("expected failure")
	}
}
