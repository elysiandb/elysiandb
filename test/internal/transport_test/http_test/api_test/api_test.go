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
