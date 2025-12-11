package api_test

import (
	"encoding/json"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
	api_controller "github.com/taymour/elysiandb/internal/transport/http/api"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Api.Cache.Enabled = false
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = false
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()

	cache.InitCache(30)
	api_storage.DeleteAll()
}

func newCtx(method, uri, body string) *fasthttp.RequestCtx {
	req := fasthttp.AcquireRequest()
	req.SetRequestURI(uri)
	req.Header.SetMethod(method)
	if body != "" {
		req.SetBodyString(body)
	}
	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	return ctx
}

func TestCreateAndGetById(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/books", `{"title":"Dune"}`)
	ctx.SetUserValue("entity", "books")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("create failed")
	}

	var obj map[string]interface{}
	json.Unmarshal(ctx.Response.Body(), &obj)
	id := obj["id"].(string)

	ctx = newCtx("GET", "/api/books/"+id, "")
	ctx.SetUserValue("entity", "books")
	ctx.SetUserValue("id", id)
	api_controller.GetByIdController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("get by id failed")
	}
}

func TestListAndCount(t *testing.T) {
	setup(t)

	for i := 0; i < 3; i++ {
		ctx := newCtx("POST", "/api/item", `{"name":"x"}`)
		ctx.SetUserValue("entity", "item")
		api_controller.CreateController(ctx)
	}

	ctx := newCtx("GET", "/api/item", "")
	ctx.SetUserValue("entity", "item")
	api_controller.ListController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("list failed")
	}

	ctx = newCtx("GET", "/api/item/count", "")
	ctx.SetUserValue("entity", "item")
	api_controller.CountController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("count failed")
	}
}

func TestUpdateById(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/profile", `{"name":"alice"}`)
	ctx.SetUserValue("entity", "profile")
	api_controller.CreateController(ctx)

	var obj map[string]interface{}
	json.Unmarshal(ctx.Response.Body(), &obj)
	id := obj["id"].(string)

	ctx = newCtx("PUT", "/api/profile/"+id, `{"name":"bob"}`)
	ctx.SetUserValue("entity", "profile")
	ctx.SetUserValue("id", id)
	api_controller.UpdateByIdController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("update failed")
	}
}

func TestUpdateList(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/e", `[{"a":1},{"a":2}]`)
	ctx.SetUserValue("entity", "e")
	api_controller.CreateController(ctx)

	ctx = newCtx("PUT", "/api/e", `[{"a":9}]`)
	ctx.SetUserValue("entity", "e")
	api_controller.UpdateListController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("update list failed")
	}
}

func TestDeleteAndDestroy(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/d", `{"x":1}`)
	ctx.SetUserValue("entity", "d")
	api_controller.CreateController(ctx)

	var obj map[string]interface{}
	json.Unmarshal(ctx.Response.Body(), &obj)
	id := obj["id"].(string)

	ctx = newCtx("DELETE", "/api/d/"+id, "")
	ctx.SetUserValue("entity", "d")
	ctx.SetUserValue("id", id)
	api_controller.DeleteByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNoContent {
		t.Fatal("delete by id failed")
	}

	ctx = newCtx("DELETE", "/api/d", "")
	ctx.SetUserValue("entity", "d")
	api_controller.DestroyController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNoContent {
		t.Fatal("destroy failed")
	}
}

func TestSchemaAPI(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/s", `{"a":1}`)
	ctx.SetUserValue("entity", "s")
	api_controller.CreateController(ctx)

	ctx = newCtx("PUT", "/api/s/schema", `{"fields":{"a":{"type":"number"}}}`)
	ctx.SetUserValue("entity", "s")
	api_controller.PutSchemaController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("put schema failed")
	}

	ctx = newCtx("GET", "/api/s/schema", "")
	ctx.SetUserValue("entity", "s")
	api_controller.GetSchemaController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("get schema failed")
	}
}

func TestImportExport(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/import", `{"x":[{"a":1},{"a":2}]}`)
	api_controller.ImportController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("import failed")
	}

	ctx = newCtx("GET", "/api/export", "")
	api_controller.ExportController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("export failed")
	}
}

func TestExistsController(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/t", `{"a":1}`)
	ctx.SetUserValue("entity", "t")
	api_controller.CreateController(ctx)

	var obj map[string]interface{}
	json.Unmarshal(ctx.Response.Body(), &obj)
	id := obj["id"].(string)

	ctx = newCtx("GET", "/api/t/"+id+"/exists", "")
	ctx.SetUserValue("entity", "t")
	ctx.SetUserValue("id", id)
	api_controller.ExistsController(ctx)

	if string(ctx.Response.Body())[11:15] != "true" {
		t.Fatal("exists failed")
	}
}

func TestMigrateController(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/m", `{"x":1}`)
	ctx.SetUserValue("entity", "m")
	api_controller.CreateController(ctx)

	ctx = newCtx("POST", "/api/m/migrate", `[{"set":[{"a":123}]}]`)
	ctx.SetUserValue("entity", "m")
	api_controller.MigrateController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("migrate failed")
	}
}

func TestCreateTypeController_OK(t *testing.T) {
	setup(t)

	body := `{"fields":{"title":"string","age":"number"}}`
	ctx := newCtx("POST", "/api/type/person", body)
	ctx.SetUserValue("entity", "person")

	api_controller.CreateTypeController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	if !api_storage.EntityTypeExists("person") {
		t.Fatalf("entity type not created")
	}

	s := api_storage.ReadEntityById("_elysiandb_core_schema", "person")
	if s == nil {
		t.Fatalf("schema not created")
	}
}

func TestCreateTypeController_InvalidJSON(t *testing.T) {
	setup(t)

	body := `{invalid`
	ctx := newCtx("POST", "/api/type/badjson", body)
	ctx.SetUserValue("entity", "badjson")

	api_controller.CreateTypeController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400, got %d", ctx.Response.StatusCode())
	}

	if api_storage.EntityTypeExists("badjson") {
		t.Fatalf("type should be rolled back on invalid json")
	}
}

func TestCreateTypeController_NoFields(t *testing.T) {
	setup(t)

	body := `{"x":1}`
	ctx := newCtx("POST", "/api/type/test", body)
	ctx.SetUserValue("entity", "test")

	api_controller.CreateTypeController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400, got %d", ctx.Response.StatusCode())
	}

	if api_storage.EntityTypeExists("test") {
		t.Fatalf("type should be rolled back when no fields provided")
	}
}

func TestCreateTypeController_AlreadyExists(t *testing.T) {
	setup(t)

	api_storage.CreateEntityType("dup")

	body := `{"fields":{"a":"string"}}`
	ctx := newCtx("POST", "/api/type/dup", body)
	ctx.SetUserValue("entity", "dup")

	api_controller.CreateTypeController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400, got %d", ctx.Response.StatusCode())
	}
}

func TestPutSchemaController_EntityNotFound(t *testing.T) {
	setup(t)

	ctx := newCtx("PUT", "/api/ghost/schema", `{"fields":{"a":{"type":"number"}}}`)
	ctx.SetUserValue("entity", "ghost")

	api_controller.PutSchemaController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNotFound {
		t.Fatalf("expected 404, got %d", ctx.Response.StatusCode())
	}
}

func TestPutSchemaController_InvalidJSON(t *testing.T) {
	setup(t)

	api_storage.CreateEntityType("x")

	ctx := newCtx("PUT", "/api/x/schema", `{invalid`)
	ctx.SetUserValue("entity", "x")

	api_controller.PutSchemaController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400, got %d", ctx.Response.StatusCode())
	}
}

func TestPutSchemaController_NoFieldsObject(t *testing.T) {
	setup(t)

	api_storage.CreateEntityType("x")

	ctx := newCtx("PUT", "/api/x/schema", `{"fields":123}`)
	ctx.SetUserValue("entity", "x")

	api_controller.PutSchemaController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400, got %d", ctx.Response.StatusCode())
	}
}

func TestPutSchemaController_OK(t *testing.T) {
	setup(t)

	api_storage.CreateEntityType("x")

	body := `{"fields":{"name":{"type":"string"}}}`
	ctx := newCtx("PUT", "/api/x/schema", body)
	ctx.SetUserValue("entity", "x")

	api_controller.PutSchemaController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	s := api_storage.GetEntitySchema("x")
	if s == nil {
		t.Fatalf("schema should exist after update")
	}
}

func TestGetEntityTypesController_Empty(t *testing.T) {
	setup(t)

	ctx := newCtx("GET", "/api/types", "")
	api_controller.GetEntityTypesController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	var out map[string]interface{}
	json.Unmarshal(ctx.Response.Body(), &out)
	entities := out["entities"].([]interface{})
	if len(entities) != 0 {
		t.Fatalf("expected empty list, got %v", entities)
	}
}

func TestGetEntityTypesController_WithSchemas(t *testing.T) {
	setup(t)

	api_storage.CreateEntityType("book")
	api_storage.UpdateEntitySchema("book", map[string]interface{}{
		"title": "string",
	})

	api_storage.CreateEntityType("user")
	api_storage.UpdateEntitySchema("user", map[string]interface{}{
		"name": "string",
	})

	ctx := newCtx("GET", "/api/types", "")
	api_controller.GetEntityTypesController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	var out map[string]interface{}
	json.Unmarshal(ctx.Response.Body(), &out)

	raw := out["entities"].([]interface{})
	valid := make([]string, 0)
	for _, v := range raw {
		if v == nil {
			continue
		}
		s := v.(string)
		if s != "null" && len(s) > 0 {
			valid = append(valid, s)
		}
	}

	if len(valid) != 2 {
		t.Fatalf("expected 2 schemas, got %v", valid)
	}

	for _, s := range valid {
		var m map[string]interface{}
		json.Unmarshal([]byte(s), &m)
		if m["id"] != "book" && m["id"] != "user" {
			t.Fatalf("unexpected schema: %v", m)
		}
	}
}

func TestGetEntityTypesController_MarshalError(t *testing.T) {
	setup(t)

	api_storage.CreateEntityType("x")

	storage.PutJsonValue(globals.ApiSingleEntityKey("_elysiandb_core_schema", "x"), map[string]interface{}{
		"bad": func() {},
	})

	ctx := newCtx("GET", "/api/types", "")
	api_controller.GetEntityTypesController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusInternalServerError {
		t.Fatalf("expected 500, got %d", ctx.Response.StatusCode())
	}
}
