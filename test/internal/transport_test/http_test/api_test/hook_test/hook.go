package http_hook_test

import (
	"encoding/json"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	http_hook "github.com/taymour/elysiandb/internal/transport/http/api/hook"
	"github.com/valyala/fasthttp"
)

func setupHookTest(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Security.Authentication.Enabled = true
	cfg.Api.Hooks.Enabled = true
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()
	api_storage.DeleteAll()

	hook.InitHooks()

	api_storage.WriteEntity("_elysiandb_core_user", map[string]any{
		"id":       "admin",
		"username": "admin",
		"role":     "admin",
	})
}

func adminCtx(method string, body []byte) *fasthttp.RequestCtx {
	s, _ := security.CreateSession("admin", security.RoleAdmin, time.Hour)

	req := fasthttp.AcquireRequest()
	req.Header.SetMethod(method)
	req.Header.SetCookie(security.SessionCookieName, s.ID)
	if body != nil {
		req.SetBody(body)
	}

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)
	return &ctx
}

func userCtx(method string, body []byte) *fasthttp.RequestCtx {
	s, _ := security.CreateSession("user", security.RoleUser, time.Hour)

	req := fasthttp.AcquireRequest()
	req.Header.SetMethod(method)
	req.Header.SetCookie(security.SessionCookieName, s.ID)
	if body != nil {
		req.SetBody(body)
	}

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)
	return &ctx
}

func TestGetHooksForEntity_OK(t *testing.T) {
	setupHookTest(t)

	_ = hook.CreateHook(hook.Hook{
		Entity:   "doc",
		Name:     "h1",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Enabled:  true,
	})

	ctx := adminCtx("GET", nil)
	ctx.SetUserValue("entity", "doc")

	http_hook.GetHooksForEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}

	var out []hook.Hook
	_ = json.Unmarshal(ctx.Response.Body(), &out)

	if len(out) != 1 {
		t.Fatalf("expected 1 hook")
	}
}

func TestGetHooksForEntity_Forbidden(t *testing.T) {
	setupHookTest(t)

	ctx := userCtx("GET", nil)
	ctx.SetUserValue("entity", "doc")

	http_hook.GetHooksForEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}

func TestCreateHookForEntity_OK(t *testing.T) {
	setupHookTest(t)

	body, _ := json.Marshal(map[string]any{
		"name":     "h1",
		"event":    hook.HookEventPostRead,
		"language": "javascript",
		"enabled":  true,
	})

	ctx := adminCtx("POST", body)
	ctx.SetUserValue("entity", "doc")

	http_hook.CreateHookForEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
}

func TestCreateHookForEntity_InvalidJSON(t *testing.T) {
	setupHookTest(t)

	ctx := adminCtx("POST", []byte("{bad"))
	ctx.SetUserValue("entity", "doc")

	http_hook.CreateHookForEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400")
	}
}

func TestCreateHookForEntity_Forbidden(t *testing.T) {
	setupHookTest(t)

	ctx := userCtx("POST", nil)
	ctx.SetUserValue("entity", "doc")

	http_hook.CreateHookForEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}

func TestGetHookById_OK(t *testing.T) {
	setupHookTest(t)

	h := hook.Hook{
		Entity:   "doc",
		Name:     "h1",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Enabled:  true,
	}
	_ = hook.CreateHook(h)

	list, _ := hook.GetHooksForEntity("doc")
	id := list[0].ID

	ctx := adminCtx("GET", nil)
	ctx.SetUserValue("id", id)

	http_hook.GetHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
}

func TestGetHookById_NotFound(t *testing.T) {
	setupHookTest(t)

	ctx := adminCtx("GET", nil)
	ctx.SetUserValue("id", "missing")

	http_hook.GetHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNotFound {
		t.Fatalf("expected 404")
	}
}

func TestGetHookById_Forbidden(t *testing.T) {
	setupHookTest(t)

	ctx := userCtx("GET", nil)
	ctx.SetUserValue("id", "x")

	http_hook.GetHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}

func TestUpdateHookById_OK(t *testing.T) {
	setupHookTest(t)

	_ = hook.CreateHook(hook.Hook{
		Entity:   "doc",
		Name:     "h1",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Enabled:  true,
	})

	list, _ := hook.GetHooksForEntity("doc")
	id := list[0].ID

	body, _ := json.Marshal(map[string]any{
		"name": "updated",
	})

	ctx := adminCtx("POST", body)
	ctx.SetUserValue("id", id)

	http_hook.UpdateHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
}

func TestUpdateHookById_InvalidJSON(t *testing.T) {
	setupHookTest(t)

	ctx := adminCtx("POST", []byte("{bad"))
	ctx.SetUserValue("id", "x")

	http_hook.UpdateHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400")
	}
}

func TestUpdateHookById_Forbidden(t *testing.T) {
	setupHookTest(t)

	ctx := userCtx("POST", nil)
	ctx.SetUserValue("id", "x")

	http_hook.UpdateHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}

func TestDeleteHookById_OK(t *testing.T) {
	setupHookTest(t)

	_ = hook.CreateHook(hook.Hook{
		Entity:   "doc",
		Name:     "h1",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Enabled:  true,
	})

	list, _ := hook.GetHooksForEntity("doc")
	id := list[0].ID

	ctx := adminCtx("DELETE", nil)
	ctx.SetUserValue("id", id)

	http_hook.DeleteHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
}

func TestDeleteHookById_NotFound(t *testing.T) {
	setupHookTest(t)

	ctx := adminCtx("DELETE", nil)
	ctx.SetUserValue("id", "missing")

	http_hook.DeleteHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNotFound {
		t.Fatalf("expected 404")
	}
}

func TestDeleteHookById_Forbidden(t *testing.T) {
	setupHookTest(t)

	ctx := userCtx("DELETE", nil)
	ctx.SetUserValue("id", "x")

	http_hook.DeleteHookByIdController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}
