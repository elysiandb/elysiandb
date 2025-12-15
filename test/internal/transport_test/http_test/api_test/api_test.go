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
	cfg.Security.Authentication.Enabled = false
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

func TestGetEntityTypesNamesController_Empty(t *testing.T) {
	setup(t)

	ctx := newCtx("GET", "/api/types", "")
	api_controller.GetEntityTypesNamesController(ctx)

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

func TestGetEntityTypesNamesController_WithEntities(t *testing.T) {
	setup(t)

	api_storage.CreateEntityType("book")
	api_storage.CreateEntityType("user")

	ctx := newCtx("GET", "/api/types", "")
	api_controller.GetEntityTypesNamesController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	var out map[string]interface{}
	json.Unmarshal(ctx.Response.Body(), &out)
	raw := out["entities"].([]interface{})

	if len(raw) != 2 {
		t.Fatalf("expected 2 entities, got %v", raw)
	}

	found := map[string]bool{}
	for _, v := range raw {
		found[v.(string)] = true
	}

	if !found["book"] || !found["user"] {
		t.Fatalf("unexpected entities list: %v", raw)
	}
}
