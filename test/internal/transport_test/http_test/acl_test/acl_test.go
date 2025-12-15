package http_acl_test

import (
	"encoding/json"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	http_acl "github.com/taymour/elysiandb/internal/transport/http/api/acl"
	"github.com/valyala/fasthttp"
)

func setupACLTest(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Security.Authentication.Enabled = true
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()

	api_storage.DeleteAll()
	api_storage.CreateEntityType("doc")
	api_storage.UpdateEntitySchema("doc", map[string]interface{}{
		"title": map[string]interface{}{
			"type": "string",
		},
	})

	api_storage.WriteEntity("_elysiandb_core_user", map[string]any{
		"id":       "admin",
		"username": "admin",
		"role":     "admin",
	})

	api_storage.WriteEntity("_elysiandb_core_acl", map[string]any{
		"id":       "admin::doc",
		"username": "admin",
		"entity":   "doc",
		"permissions": map[string]bool{
			"create":        true,
			"read":          true,
			"update":        true,
			"delete":        true,
			"owning_read":   true,
			"owning_write":  true,
			"owning_delete": true,
			"owning_update": true,
		},
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

func userCtx(method string) *fasthttp.RequestCtx {
	s, _ := security.CreateSession("user", security.RoleUser, time.Hour)

	req := fasthttp.AcquireRequest()
	req.Header.SetMethod(method)
	req.Header.SetCookie(security.SessionCookieName, s.ID)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)
	return &ctx
}

func TestGetACLForUsernameAndEntity_OK(t *testing.T) {
	setupACLTest(t)

	ctx := adminCtx("GET", nil)
	ctx.SetUserValue("entity", "doc")
	ctx.SetUserValue("user_name", "admin")

	http_acl.GetACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
}

func TestGetACLForUsernameAndEntity_NotFound(t *testing.T) {
	setupACLTest(t)

	ctx := adminCtx("GET", nil)
	ctx.SetUserValue("entity", "missing")
	ctx.SetUserValue("user_name", "admin")

	http_acl.GetACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNotFound {
		t.Fatalf("expected 404")
	}
}

func TestGetACLForUsernameAndEntity_Forbidden(t *testing.T) {
	setupACLTest(t)

	ctx := userCtx("GET")
	ctx.SetUserValue("entity", "doc")
	ctx.SetUserValue("user_name", "admin")

	http_acl.GetACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}

func TestGetAllACLForUsername_OK(t *testing.T) {
	setupACLTest(t)

	ctx := adminCtx("GET", nil)
	ctx.SetUserValue("user_name", "admin")

	http_acl.GetAllACLForUsername(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}

	var out []map[string]any
	_ = json.Unmarshal(ctx.Response.Body(), &out)

	if len(out) != 1 {
		t.Fatalf("expected 1 acl")
	}
}

func TestGetAllACLForUsername_Forbidden(t *testing.T) {
	setupACLTest(t)

	ctx := userCtx("GET")
	ctx.SetUserValue("user_name", "admin")

	http_acl.GetAllACLForUsername(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}

func TestUpdateACLForUsernameAndEntity_OK(t *testing.T) {
	setupACLTest(t)

	body, _ := json.Marshal(map[string]any{
		"permissions": map[string]bool{
			"read": true,
		},
	})

	ctx := adminCtx("POST", body)
	ctx.SetUserValue("entity", "doc")
	ctx.SetUserValue("user_name", "admin")

	http_acl.UpdateACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
}

func TestUpdateACLForUsernameAndEntity_InvalidJSON(t *testing.T) {
	setupACLTest(t)

	ctx := adminCtx("POST", []byte("{bad"))
	ctx.SetUserValue("entity", "doc")
	ctx.SetUserValue("user_name", "admin")

	http_acl.UpdateACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400")
	}
}

func TestUpdateACLForUsernameAndEntity_Forbidden(t *testing.T) {
	setupACLTest(t)

	ctx := userCtx("POST")
	ctx.SetUserValue("entity", "doc")
	ctx.SetUserValue("user_name", "admin")

	http_acl.UpdateACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}

func TestSetDefaultACLForUsernameAndEntity_OK(t *testing.T) {
	setupACLTest(t)

	ctx := adminCtx("POST", nil)
	ctx.SetUserValue("entity", "doc")
	ctx.SetUserValue("user_name", "admin")

	http_acl.SetDefaultACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
}

func TestSetDefaultACLForUsernameAndEntity_BadRequest(t *testing.T) {
	setupACLTest(t)

	ctx := adminCtx("POST", nil)
	ctx.SetUserValue("entity", "doc")
	ctx.SetUserValue("user_name", "ghost")

	http_acl.SetDefaultACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400")
	}
}

func TestSetDefaultACLForUsernameAndEntity_Forbidden(t *testing.T) {
	setupACLTest(t)

	ctx := userCtx("POST")
	ctx.SetUserValue("entity", "doc")
	ctx.SetUserValue("user_name", "admin")

	http_acl.SetDefaultACLForUsernameAndEntityController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
}
