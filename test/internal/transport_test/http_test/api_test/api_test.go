package api_test

import (
	"encoding/hex"
	"encoding/json"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	api_controller "github.com/taymour/elysiandb/internal/transport/http/api"
	api_transaction "github.com/taymour/elysiandb/internal/transport/http/api/transactions"
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

func newUserCtx(username, method, uri, body string) *fasthttp.RequestCtx {
	ctx := newCtx(method, uri, body)
	ctx.SetUserValue("username", username)

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

func TestUpdateById_KeepsStrictManualSchema(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Schema.Strict = true
	globals.SetConfig(cfg)

	ctx := newCtx("POST", "/api/oeuvre/create", `{"fields":{"titre":{"type":"string","required":true},"annee":{"type":"number","required":false}}}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateTypeController(ctx)

	ctx = newCtx("POST", "/api/oeuvre", `{"id":"o1","titre":"Alien"}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on create, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	ctx = newCtx("PUT", "/api/oeuvre/o1", `{"titre":"Alien (1979)"}`)
	ctx.SetUserValue("entity", "oeuvre")
	ctx.SetUserValue("id", "o1")
	api_controller.UpdateByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on update, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	ctx = newCtx("POST", "/api/oeuvre", `{"titre":"X","note":9}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400 for undeclared field after update, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	ctx = newCtx("POST", "/api/oeuvre", `{"annee":1979}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400 for missing required field after update, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
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

func TestCreate_StrictManualSchemaWithUserAuthentication(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Schema.Strict = true
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
	globals.SetConfig(cfg)

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatalf("admin user creation failed: %v", err)
	}

	ctx := newUserCtx(security.DefaultAdminUsername, "POST", "/api/oeuvre/create", `{"fields":{"titre":{"type":"string","required":true},"annee":{"type":"number","required":false}}}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateTypeController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on type creation, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/oeuvre", `{"titre":"Alien","annee":1979}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	var obj map[string]any
	json.Unmarshal(ctx.Response.Body(), &obj)
	if obj[acl.UsernameField] != "admin" {
		t.Fatalf("expected owner admin, got %v", obj[acl.UsernameField])
	}

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/oeuvre", `[{"titre":"Alien"},{"titre":"Aliens","annee":1986}]`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on list creation, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/oeuvre", `{"titre":"X","note":9}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400 for undeclared field, got %d", ctx.Response.StatusCode())
	}
}

func TestUpdate_StrictManualSchemaWithUserAuthenticationKeepsSchema(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Schema.Strict = true
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
	globals.SetConfig(cfg)

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatalf("admin user creation failed: %v", err)
	}

	assertManualSchema := func(step string) {
		ctx := newUserCtx(security.DefaultAdminUsername, "GET", "/api/oeuvre/schema", "")
		ctx.SetUserValue("entity", "oeuvre")
		api_controller.GetSchemaController(ctx)

		var sc map[string]any
		json.Unmarshal(ctx.Response.Body(), &sc)
		if sc["_manual"] != true {
			t.Fatalf("manual schema lost after %s: %s", step, ctx.Response.Body())
		}

		fields, _ := sc["fields"].(map[string]any)
		titre, _ := fields["titre"].(map[string]any)
		annee, _ := fields["annee"].(map[string]any)
		if titre["required"] != true || annee["required"] != false {
			t.Fatalf("required flags changed after %s: %s", step, ctx.Response.Body())
		}
	}

	ctx := newUserCtx(security.DefaultAdminUsername, "POST", "/api/oeuvre/create", `{"fields":{"titre":{"type":"string","required":true},"annee":{"type":"number","required":false}}}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateTypeController(ctx)

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/oeuvre", `{"id":"o1","titre":"Alien","annee":1979}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on create, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	ctx = newUserCtx(security.DefaultAdminUsername, "PUT", "/api/oeuvre/o1", `{"titre":"Alien","annee":1980}`)
	ctx.SetUserValue("entity", "oeuvre")
	ctx.SetUserValue("id", "o1")
	api_controller.UpdateByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on update, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	assertManualSchema("PUT")

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/tx/begin", "")
	api_transaction.BeginTransactionController(ctx)

	var begin map[string]string
	json.Unmarshal(ctx.Response.Body(), &begin)
	txID := begin["transaction_id"]

	ctx = newUserCtx(security.DefaultAdminUsername, "PUT", "/api/tx/"+txID+"/entity/oeuvre/o1", `{"annee":1990}`)
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "oeuvre")
	ctx.SetUserValue("id", "o1")
	api_transaction.UpdateTransactionController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on transactional update, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/tx/"+txID+"/commit", "")
	ctx.SetUserValue("txId", txID)
	api_transaction.CommitTransactionController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200 on commit, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	assertManualSchema("transaction commit")

	stored := api_storage.ReadEntityById("oeuvre", "o1")
	if stored["annee"] != float64(1990) || stored[acl.UsernameField] != security.DefaultAdminUsername {
		t.Fatalf("expected updated document owned by admin, got %v", stored)
	}

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/oeuvre", `{"titre":"Y","note":9}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400 for undeclared field after updates, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/oeuvre", `{"annee":2000}`)
	ctx.SetUserValue("entity", "oeuvre")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400 for missing required field after updates, got %d: %s", ctx.Response.StatusCode(), ctx.Response.Body())
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

func TestGetById_WithPostReadHook(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Hooks.Enabled = true
	globals.SetConfig(cfg)

	api_storage.CreateEntityType("book")

	api_storage.WriteEntity("_elysiandb_core_hook", map[string]any{
		"id":         "h1",
		"name":       "post_read",
		"entity":     "book",
		"event":      "post_read",
		"language":   "javascript",
		"priority":   1,
		"enabled":    true,
		"bypass_acl": true,
		"script": `
function postRead(ctx) {
  ctx.entity.hooked = true
  return ctx.entity
}
`,
	})

	ctx := newCtx("POST", "/api/book", `{"title":"Dune"}`)
	ctx.SetUserValue("entity", "book")
	api_controller.CreateController(ctx)

	var obj map[string]any
	json.Unmarshal(ctx.Response.Body(), &obj)
	id := obj["id"].(string)

	ctx = newCtx("GET", "/api/book/"+id, "")
	ctx.SetUserValue("entity", "book")
	ctx.SetUserValue("id", id)
	api_controller.GetByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}

	json.Unmarshal(ctx.Response.Body(), &obj)

	if obj["hooked"] != true {
		t.Fatalf("hook not applied")
	}
}

func TestList_WithPostReadHook(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Hooks.Enabled = true
	globals.SetConfig(cfg)

	api_storage.CreateEntityType("item")

	api_storage.WriteEntity("_elysiandb_core_hook", map[string]any{
		"id":         "h2",
		"name":       "post_read",
		"entity":     "item",
		"event":      "post_read",
		"language":   "javascript",
		"priority":   1,
		"enabled":    true,
		"bypass_acl": true,
		"script": `
function postRead(ctx) {
  ctx.entity.fromHook = "yes"
  return ctx.entity
}
`,
	})

	for i := 0; i < 2; i++ {
		ctx := newCtx("POST", "/api/item", `{"a":1}`)
		ctx.SetUserValue("entity", "item")
		api_controller.CreateController(ctx)
	}

	ctx := newCtx("GET", "/api/item", "")
	ctx.SetUserValue("entity", "item")
	api_controller.ListController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}

	var list []map[string]any
	json.Unmarshal(ctx.Response.Body(), &list)

	if len(list) != 2 {
		t.Fatalf("expected 2 items")
	}

	for _, it := range list {
		if it["fromHook"] != "yes" {
			t.Fatalf("hook not applied on list")
		}
	}
}

func TestList_WithoutHooksEnabled(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Hooks.Enabled = false
	globals.SetConfig(cfg)

	api_storage.CreateEntityType("plain")

	api_storage.WriteEntity("_elysiandb_core_hook", map[string]any{
		"id":         "h3",
		"name":       "post_read",
		"entity":     "plain",
		"event":      "post_read",
		"language":   "javascript",
		"priority":   1,
		"enabled":    true,
		"bypass_acl": true,
		"script": `
function postRead(ctx) {
  ctx.entity.x = 1
  return ctx.entity
}
`,
	})

	ctx := newCtx("POST", "/api/plain", `{"a":1}`)
	ctx.SetUserValue("entity", "plain")
	api_controller.CreateController(ctx)

	ctx = newCtx("GET", "/api/plain", "")
	ctx.SetUserValue("entity", "plain")
	api_controller.ListController(ctx)

	var out []map[string]any
	json.Unmarshal(ctx.Response.Body(), &out)

	if _, ok := out[0]["x"]; ok {
		t.Fatalf("hook should not be applied when disabled")
	}
}

func TestGetById_NoHookForEntity(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Hooks.Enabled = true
	globals.SetConfig(cfg)

	api_storage.CreateEntityType("nohook")

	ctx := newCtx("POST", "/api/nohook", `{"a":1}`)
	ctx.SetUserValue("entity", "nohook")
	api_controller.CreateController(ctx)

	var obj map[string]any
	json.Unmarshal(ctx.Response.Body(), &obj)
	id := obj["id"].(string)

	ctx = newCtx("GET", "/api/nohook/"+id, "")
	ctx.SetUserValue("entity", "nohook")
	ctx.SetUserValue("id", id)
	api_controller.GetByIdController(ctx)

	json.Unmarshal(ctx.Response.Body(), &obj)

	if _, ok := obj["hooked"]; ok {
		t.Fatalf("unexpected hook application")
	}
}

func TestList_WithPreReadHookFiltering(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Hooks.Enabled = true
	globals.SetConfig(cfg)

	api_storage.CreateEntityType("task")

	api_storage.WriteEntity("_elysiandb_core_hook", map[string]any{
		"id":         "h_pre_1",
		"name":       "pre_read",
		"entity":     "task",
		"event":      "pre_read",
		"language":   "javascript",
		"priority":   1,
		"enabled":    true,
		"bypass_acl": true,
		"script": `
function preRead(ctx) {
  ctx.entity.allowed = ctx.entity.value === 1
  return ctx.entity
}
`,
	})

	ctx := newCtx("POST", "/api/task", `{"value":1}`)
	ctx.SetUserValue("entity", "task")
	api_controller.CreateController(ctx)

	ctx = newCtx("POST", "/api/task", `{"value":2}`)
	ctx.SetUserValue("entity", "task")
	api_controller.CreateController(ctx)

	ctx = newCtx("GET", "/api/task?filter[allowed][eq]=true", "")
	ctx.SetUserValue("entity", "task")
	api_controller.ListController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}

	var list []map[string]any
	json.Unmarshal(ctx.Response.Body(), &list)

	if len(list) != 1 {
		t.Fatalf("expected 1 item after pre_read filtering")
	}

	if list[0]["value"] != float64(1) {
		t.Fatalf("unexpected filtered item")
	}
}

func TestList_PreReadHookIgnoredWhenDisabled(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Hooks.Enabled = false
	globals.SetConfig(cfg)

	api_storage.CreateEntityType("preoff")

	api_storage.WriteEntity("_elysiandb_core_hook", map[string]any{
		"id":         "h_pre_2",
		"name":       "pre_read",
		"entity":     "preoff",
		"event":      "pre_read",
		"language":   "javascript",
		"priority":   1,
		"enabled":    true,
		"bypass_acl": true,
		"script": `
function preRead(ctx) {
  ctx.entity.flag = true
  return ctx.entity
}
`,
	})

	ctx := newCtx("POST", "/api/preoff", `{"a":1}`)
	ctx.SetUserValue("entity", "preoff")
	api_controller.CreateController(ctx)

	ctx = newCtx("GET", "/api/preoff?filter[flag][eq]=true", "")
	ctx.SetUserValue("entity", "preoff")
	api_controller.ListController(ctx)

	var list []map[string]any
	json.Unmarshal(ctx.Response.Body(), &list)

	if len(list) != 0 {
		t.Fatalf("expected no item when pre_read is disabled, got %v", list)
	}
}

func TestList_PreReadHookFilteringExcludesMissingStoredField(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Hooks.Enabled = true
	globals.SetConfig(cfg)

	api_storage.CreateEntityType("ticket")

	api_storage.WriteEntity("_elysiandb_core_hook", map[string]any{
		"id":         "h_pre_4",
		"name":       "pre_read",
		"entity":     "ticket",
		"event":      "pre_read",
		"language":   "javascript",
		"priority":   1,
		"enabled":    true,
		"bypass_acl": true,
		"script": `
function preRead(ctx) {
  ctx.entity.allowed = ctx.entity.value === 1
  return ctx.entity
}
`,
	})

	for _, body := range []string{`{"value":1,"tag":"a"}`, `{"value":1}`, `{"value":2,"tag":"a"}`} {
		ctx := newCtx("POST", "/api/ticket", body)
		ctx.SetUserValue("entity", "ticket")
		api_controller.CreateController(ctx)
	}

	ctx := newCtx("GET", "/api/ticket?filter[allowed][eq]=true&filter[tag][eq]=a", "")
	ctx.SetUserValue("entity", "ticket")
	api_controller.ListController(ctx)

	var list []map[string]any
	json.Unmarshal(ctx.Response.Body(), &list)

	if len(list) != 1 || list[0]["value"] != float64(1) || list[0]["tag"] != "a" {
		t.Fatalf("expected only the allowed ticket tagged a, got %v", list)
	}
}

func TestList_PreReadAndPostReadTogether(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Hooks.Enabled = true
	globals.SetConfig(cfg)

	api_storage.CreateEntityType("combo")

	api_storage.WriteEntity("_elysiandb_core_hook", map[string]any{
		"id":         "h_pre_3",
		"name":       "pre_read",
		"entity":     "combo",
		"event":      "pre_read",
		"language":   "javascript",
		"priority":   2,
		"enabled":    true,
		"bypass_acl": true,
		"script": `
function preRead(ctx) {
  ctx.entity.keep = ctx.entity.x === 1
  return ctx.entity
}
`,
	})

	api_storage.WriteEntity("_elysiandb_core_hook", map[string]any{
		"id":         "h_post_3",
		"name":       "post_read",
		"entity":     "combo",
		"event":      "post_read",
		"language":   "javascript",
		"priority":   1,
		"enabled":    true,
		"bypass_acl": true,
		"script": `
function postRead(ctx) {
  ctx.entity.after = "ok"
  return ctx.entity
}
`,
	})

	ctx := newCtx("POST", "/api/combo", `{"x":1}`)
	ctx.SetUserValue("entity", "combo")
	api_controller.CreateController(ctx)

	ctx = newCtx("POST", "/api/combo", `{"x":2}`)
	ctx.SetUserValue("entity", "combo")
	api_controller.CreateController(ctx)

	ctx = newCtx("GET", "/api/combo?filter[keep][eq]=true", "")
	ctx.SetUserValue("entity", "combo")
	api_controller.ListController(ctx)

	var list []map[string]any
	json.Unmarshal(ctx.Response.Body(), &list)

	if len(list) != 1 {
		t.Fatalf("expected 1 item")
	}

	if list[0]["after"] != "ok" {
		t.Fatalf("post_read hook not applied")
	}
}

func TestQueryController_BasicQuery(t *testing.T) {
	setup(t)

	api_storage.WriteEntity("book", map[string]any{"id": "1", "title": "Go"})
	api_storage.WriteEntity("book", map[string]any{"id": "2", "title": "Rust"})

	body := `{
		"entity":"book",
		"filters":{"title":{"eq":"Go"}}
	}`

	ctx := newCtx("POST", "/api/query", body)
	api_controller.QueryController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}

	var out []map[string]any
	json.Unmarshal(ctx.Response.Body(), &out)

	if len(out) != 1 || out[0]["id"] != "1" {
		t.Fatalf("unexpected result %v", out)
	}
}

func TestQueryController_CountOnly(t *testing.T) {
	setup(t)

	for i := 0; i < 3; i++ {
		api_storage.WriteEntity("x", map[string]any{"id": string(rune('a' + i))})
	}

	body := `{
		"entity":"x",
		"countOnly":true,
		"filters":{"id":{"neq":"__never__"}}
	}`

	ctx := newCtx("POST", "/api/query", body)
	api_controller.QueryController(ctx)

	var out map[string]int64
	json.Unmarshal(ctx.Response.Body(), &out)

	if out["count"] != 3 {
		t.Fatalf("expected count 3, got %v", out)
	}
}

func TestQueryController_FieldsFiltering(t *testing.T) {
	setup(t)

	api_storage.WriteEntity("f", map[string]any{
		"id":    "1",
		"title": "Go",
		"meta":  map[string]any{"x": 1},
	})

	body := `{
		"entity":"f",
		"fields":"id,title",
		"filters":{"id":{"eq":"1"}}
	}`

	ctx := newCtx("POST", "/api/query", body)
	api_controller.QueryController(ctx)

	var out []map[string]any
	json.Unmarshal(ctx.Response.Body(), &out)

	if len(out) != 1 {
		t.Fatalf("expected 1 result, got %v", out)
	}

	if _, ok := out[0]["meta"]; ok {
		t.Fatalf("meta should be filtered out")
	}
}

func TestQueryController_InvalidJSON(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/query", `{invalid`)
	api_controller.QueryController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400")
	}
}

func TestQueryController_InvalidFilter(t *testing.T) {
	setup(t)

	body := `{
		"entity":"x",
		"filters":{"or":"nope"}
	}`

	ctx := newCtx("POST", "/api/query", body)
	api_controller.QueryController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400")
	}
}

func TestQueryController_CacheHitAndMiss(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Api.Cache.Enabled = true
	cfg.Api.Hooks.Enabled = false
	globals.SetConfig(cfg)

	api_storage.DeleteAllEntities("_elysiandb_core_hook")
	api_storage.DeleteEntityType("_elysiandb_core_hook")

	api_storage.WriteEntity("cachetest", map[string]any{
		"id": "1",
		"v":  1,
	})

	body := `{
		"entity":"cachetest",
		"filters":{"id":{"eq":"1"}}
	}`

	ctx1 := newCtx("POST", "/api/query", body)
	api_controller.QueryController(ctx1)

	ctx2 := newCtx("POST", "/api/query", body)
	api_controller.QueryController(ctx2)

	if string(ctx2.Response.Header.Peek("X-Elysian-Cache")) != "HIT" {
		t.Fatalf("expected cache HIT")
	}
}

func TestQueryPayload_HashDeterministic(t *testing.T) {
	p := api_controller.QueryPayload{
		Entity: "a",
		Filters: map[string]any{
			"or": []any{
				map[string]any{"x": map[string]any{"eq": "1"}},
				map[string]any{"y": map[string]any{"eq": "2"}},
			},
		},
		Sorts: map[string]string{"b": "asc", "a": "desc"},
	}

	h1 := hex.EncodeToString(p.Hash())
	h2 := hex.EncodeToString(p.Hash())

	if h1 != h2 {
		t.Fatalf("hash must be stable, got %s vs %s", h1, h2)
	}
}

func setupOwnership(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"

	for _, username := range []string{"alice", "bob"} {
		perms := acl.NewPermissions()
		perms[acl.PermissionCreate] = true
		perms[acl.PermissionOwningRead] = true
		perms[acl.PermissionOwningUpdate] = true
		a := acl.ACL{Username: username, Entity: "notes", Permissions: perms}
		api_storage.WriteEntity(acl.ACLEntity, a.ToDataMap())
	}
}

func createNotesAs(username, body string) *fasthttp.RequestCtx {
	ctx := newUserCtx(username, "POST", "/api/notes", body)
	ctx.SetUserValue("entity", "notes")
	api_controller.CreateController(ctx)

	return ctx
}

func createNoteAs(t *testing.T, username, body string) string {
	ctx := createNotesAs(username, body)
	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("create failed: %d", ctx.Response.StatusCode())
	}

	var obj map[string]any
	json.Unmarshal(ctx.Response.Body(), &obj)

	return obj["id"].(string)
}

func updateNoteAs(username, id, body string) *fasthttp.RequestCtx {
	ctx := newUserCtx(username, "PUT", "/api/notes/"+id, body)
	ctx.SetUserValue("entity", "notes")
	ctx.SetUserValue("id", id)
	api_controller.UpdateByIdController(ctx)

	return ctx
}

func updateNotesAs(username, body string) *fasthttp.RequestCtx {
	ctx := newUserCtx(username, "PUT", "/api/notes", body)
	ctx.SetUserValue("entity", "notes")
	api_controller.UpdateListController(ctx)

	return ctx
}

func assertNote(t *testing.T, id, owner, title string) {
	t.Helper()

	stored := api_storage.ReadEntityById("notes", id)
	if stored == nil {
		t.Fatalf("note %s not found", id)
	}

	if stored[acl.UsernameField] != owner || stored["title"] != title {
		t.Fatalf("unexpected note %s: %v", id, stored)
	}
}

func TestUpdateById_OwningUpdateRejectsOtherUsersEntity(t *testing.T) {
	setupOwnership(t)

	id := createNoteAs(t, "alice", `{"title":"alice"}`)

	ctx := updateNoteAs("bob", id, `{"title":"bob","`+acl.UsernameField+`":"bob"}`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	ctx = updateNoteAs("bob", id, `{"title":"bob"}`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	assertNote(t, id, "alice", "alice")
}

func TestUpdateById_OwnerCanUpdateWithoutUsernameField(t *testing.T) {
	setupOwnership(t)

	id := createNoteAs(t, "alice", `{"title":"alice"}`)

	ctx := updateNoteAs("alice", id, `{"title":"v2"}`)
	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	assertNote(t, id, "alice", "v2")
}

func TestUpdateById_OwningUpdateCannotChangeOwner(t *testing.T) {
	setupOwnership(t)

	id := createNoteAs(t, "alice", `{"title":"alice"}`)

	ctx := updateNoteAs("alice", id, `{"title":"v2","`+acl.UsernameField+`":"bob"}`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	assertNote(t, id, "alice", "alice")
}

func TestUpdateById_IdInPayloadDoesNotRedirectWrite(t *testing.T) {
	setupOwnership(t)

	aliceID := createNoteAs(t, "alice", `{"title":"alice"}`)
	bobID := createNoteAs(t, "bob", `{"title":"bob"}`)

	ctx := updateNoteAs("bob", bobID, `{"id":"`+aliceID+`","title":"bob v2","`+acl.UsernameField+`":"bob"}`)
	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	assertNote(t, aliceID, "alice", "alice")
	assertNote(t, bobID, "bob", "bob v2")
}

func TestUpdateList_OwningUpdateRejectsOtherUsersEntity(t *testing.T) {
	setupOwnership(t)

	aliceID := createNoteAs(t, "alice", `{"title":"alice"}`)
	bobID := createNoteAs(t, "bob", `{"title":"bob"}`)

	ctx := updateNotesAs("bob", `[{"id":"`+bobID+`","title":"bob v2"},{"id":"`+aliceID+`","title":"bob","`+acl.UsernameField+`":"bob"}]`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	assertNote(t, aliceID, "alice", "alice")
	assertNote(t, bobID, "bob", "bob")
}

func TestUpdateList_OwnerCanUpdateWithoutUsernameField(t *testing.T) {
	setupOwnership(t)

	id := createNoteAs(t, "alice", `{"title":"alice"}`)

	ctx := updateNotesAs("alice", `[{"id":"`+id+`","title":"v2"}]`)
	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	assertNote(t, id, "alice", "v2")
}

func newSessionCtx(t *testing.T, username, method, uri, body string) *fasthttp.RequestCtx {
	session, err := security.CreateSession(username, security.RoleUser, time.Hour)
	if err != nil {
		t.Fatalf("session creation failed: %v", err)
	}

	ctx := newCtx(method, uri, body)
	ctx.Request.Header.SetCookie(security.SessionCookieName, session.ID)

	return ctx
}

func runDuringConcurrentLogin(first *fasthttp.RequestCtx, handler fasthttp.RequestHandler, second *fasthttp.RequestCtx) {
	firstAuthenticated := make(chan struct{})
	secondAuthenticated := make(chan struct{})
	done := make(chan struct{})

	go func() {
		defer close(done)

		security.UserAuth(func(ctx *fasthttp.RequestCtx) {
			close(firstAuthenticated)
			<-secondAuthenticated
			handler(ctx)
		})(first)
	}()

	<-firstAuthenticated

	security.UserAuth(func(ctx *fasthttp.RequestCtx) {
		close(secondAuthenticated)
	})(second)

	<-done
}

func TestCreate_OwnerComesFromOwnRequestDuringConcurrentLogin(t *testing.T) {
	setupOwnership(t)

	aliceCtx := newSessionCtx(t, "alice", "POST", "/api/notes", `{"title":"alice"}`)
	aliceCtx.SetUserValue("entity", "notes")
	bobCtx := newSessionCtx(t, "bob", "GET", "/api/notes", "")

	runDuringConcurrentLogin(aliceCtx, api_controller.CreateController, bobCtx)

	if aliceCtx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200, got %d", aliceCtx.Response.StatusCode())
	}

	var obj map[string]any
	json.Unmarshal(aliceCtx.Response.Body(), &obj)
	if obj[acl.UsernameField] != "alice" {
		t.Fatalf("expected owner alice, got %v", obj[acl.UsernameField])
	}
}

func TestUpdateById_ACLUsesOwnRequestUserDuringConcurrentLogin(t *testing.T) {
	setupOwnership(t)

	api_storage.WriteEntity("notes", map[string]any{
		"id":              "n1",
		"title":           "alice",
		acl.UsernameField: "alice",
	})

	bobCtx := newSessionCtx(t, "bob", "PUT", "/api/notes/n1", `{"title":"bob"}`)
	bobCtx.SetUserValue("entity", "notes")
	bobCtx.SetUserValue("id", "n1")
	aliceCtx := newSessionCtx(t, "alice", "GET", "/api/notes", "")

	runDuringConcurrentLogin(bobCtx, api_controller.UpdateByIdController, aliceCtx)

	if bobCtx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", bobCtx.Response.StatusCode())
	}

	assertNote(t, "n1", "alice", "alice")
}

func TestCreate_ExistingIdOfOtherUserIsForbidden(t *testing.T) {
	setupOwnership(t)

	id := createNoteAs(t, "alice", `{"title":"alice"}`)

	ctx := createNotesAs("bob", `{"id":"`+id+`","title":"bob"}`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	ctx = createNotesAs("bob", `[{"title":"bob new"},{"id":"`+id+`","title":"bob"}]`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403 on list, got %d", ctx.Response.StatusCode())
	}

	assertNote(t, id, "alice", "alice")

	if n := len(api_storage.ListEntities("notes", 0, 0, "", true, nil, "", "")); n != 1 {
		t.Fatalf("expected 1 note, got %d", n)
	}
}

func TestCreate_OwnerCanRewriteOwnEntity(t *testing.T) {
	setupOwnership(t)

	id := createNoteAs(t, "alice", `{"title":"alice"}`)

	ctx := createNotesAs("alice", `{"id":"`+id+`","title":"v2"}`)
	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	assertNote(t, id, "alice", "v2")
}

func TestCreate_RequiresCreatePermission(t *testing.T) {
	setupOwnership(t)

	perms := acl.NewPermissions()
	perms[acl.PermissionOwningRead] = true
	perms[acl.PermissionOwningUpdate] = true
	a := acl.ACL{Username: "carol", Entity: "notes", Permissions: perms}
	api_storage.WriteEntity(acl.ACLEntity, a.ToDataMap())

	ctx := createNotesAs("carol", `{"title":"carol"}`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	ctx = createNotesAs("carol", `[{"title":"carol"}]`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403 on list, got %d", ctx.Response.StatusCode())
	}

	if n := len(api_storage.ListEntities("notes", 0, 0, "", true, nil, "", "")); n != 0 {
		t.Fatalf("expected no note, got %d", n)
	}
}

func TestCreate_CoreEntityIsForbidden(t *testing.T) {
	setupOwnership(t)

	ctx := newUserCtx("bob", "POST", "/api/"+acl.ACLEntity, `{"id":"bob::notes","username":"bob","entity":"notes","permissions":{"update":true}}`)
	ctx.SetUserValue("entity", acl.ACLEntity)
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	if acl.GetACLEntityForUsername("notes", "bob").Can(acl.PermissionUpdate) {
		t.Fatal("acl was modified")
	}
}

func TestCreate_NewEntityTypeUsesRoleDefaults(t *testing.T) {
	setupOwnership(t)

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatalf("admin user creation failed: %v", err)
	}

	err := security.CreateBasicUser(&security.BasicUser{Username: "carol", Password: "pw", Role: security.RoleUser})
	if err != nil {
		t.Fatalf("user creation failed: %v", err)
	}

	ctx := newUserCtx("carol", "POST", "/api/fresh", `{"title":"carol"}`)
	ctx.SetUserValue("entity", "fresh")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403 for regular user, got %d", ctx.Response.StatusCode())
	}

	ctx = newUserCtx(security.DefaultAdminUsername, "POST", "/api/fresh", `{"title":"admin"}`)
	ctx.SetUserValue("entity", "fresh")
	api_controller.CreateController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200 for admin, got %d", ctx.Response.StatusCode())
	}
}
