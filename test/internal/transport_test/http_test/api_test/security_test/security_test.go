package http_security_test

import (
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	http_security "github.com/taymour/elysiandb/internal/transport/http/api/security"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()

	_ = security.InitBasicUsersStorage()
	_ = security.InitAdminUserIfNotExists()
}

func newCtx(method, uri, body string, session *security.Session) *fasthttp.RequestCtx {
	req := fasthttp.AcquireRequest()
	req.SetRequestURI(uri)
	req.Header.SetMethod(method)
	if body != "" {
		req.SetBodyString(body)
	}
	if session != nil {
		req.Header.SetCookie(security.SessionCookieName, session.ID)
	}
	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	return ctx
}

func login(t *testing.T, username string, role security.Role) *security.Session {
	s, err := security.CreateSession(username, role, time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	return s
}

func TestGetUsersController_AdminOK(t *testing.T) {
	setup(t)

	_ = security.CreateBasicUser(&security.BasicUser{Username: "u1", Password: "p1", Role: security.RoleUser})

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("GET", "/api/security/user", "", s)

	http_security.GetUsersController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatal("expected 200")
	}
}

func TestGetUsersController_Forbidden(t *testing.T) {
	setup(t)

	s := login(t, "user", security.RoleUser)
	ctx := newCtx("GET", "/api/security/user", "", s)

	http_security.GetUsersController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatal("expected 403")
	}
}

func TestGetUserByUsernameController_AdminOK(t *testing.T) {
	setup(t)

	_ = security.CreateBasicUser(&security.BasicUser{Username: "bob", Password: "x", Role: security.RoleUser})

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("GET", "/api/security/user/bob", "", s)
	ctx.SetUserValue("user_name", "bob")

	http_security.GetUserByUsernameController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatal("expected 200")
	}
}

func TestGetUserByUsernameController_ForbiddenSelf(t *testing.T) {
	setup(t)

	_ = security.CreateBasicUser(&security.BasicUser{Username: "bob", Password: "x", Role: security.RoleUser})

	s := login(t, "bob", security.RoleUser)
	ctx := newCtx("GET", "/api/security/user/bob", "", s)
	ctx.SetUserValue("user_name", "bob")

	http_security.GetUserByUsernameController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatal("expected 403")
	}
}

func TestCreateUserController_AdminOK(t *testing.T) {
	setup(t)

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)

	body := `{"username":"alice","password":"pwd","role":"user"}`
	ctx := newCtx("POST", "/api/security/user", body, s)

	http_security.CreateUserController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatal("expected 200")
	}

	if _, err := security.GetBasicUserByUsername("alice"); err != nil {
		t.Fatal("user not created")
	}
}

func TestCreateUserController_Forbidden(t *testing.T) {
	setup(t)

	s := login(t, "user", security.RoleUser)

	body := `{"username":"alice","password":"pwd","role":"user"}`
	ctx := newCtx("POST", "/api/security/user", body, s)

	http_security.CreateUserController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatal("expected 403")
	}
}

func TestCreateUserController_InvalidJSON(t *testing.T) {
	setup(t)

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)

	ctx := newCtx("POST", "/api/security/user", `{bad`, s)

	http_security.CreateUserController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatal("expected 400")
	}
}

func TestDeleteUserByUsernameController_OK(t *testing.T) {
	setup(t)

	_ = security.CreateBasicUser(&security.BasicUser{Username: "todelete", Password: "x", Role: security.RoleUser})

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("DELETE", "/api/security/user/todelete", "", s)
	ctx.SetUserValue("user_name", "todelete")

	http_security.DeleteUserByUsernameController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatal("expected 200")
	}

	if _, err := security.GetBasicUserByUsername("todelete"); err == nil {
		t.Fatal("user should be deleted")
	}
}

func TestDeleteUserByUsernameController_ForbiddenSelf(t *testing.T) {
	setup(t)

	s := login(t, "bob", security.RoleUser)
	ctx := newCtx("DELETE", "/api/security/user/bob", "", s)
	ctx.SetUserValue("user_name", "bob")

	http_security.DeleteUserByUsernameController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatal("expected 403")
	}
}

func TestChangeUserPasswordController_OK(t *testing.T) {
	setup(t)

	_ = security.CreateBasicUser(&security.BasicUser{Username: "john", Password: "old", Role: security.RoleUser})

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("PUT", "/api/security/user/john/password", `{"password":"new"}`, s)
	ctx.SetUserValue("user_name", "john")

	http_security.ChangeUserPasswordController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatal("expected 200")
	}

	if _, ok := security.AuthenticateUser("john", "new"); !ok {
		t.Fatal("password not changed")
	}
}

func TestChangeUserPasswordController_InvalidJSON(t *testing.T) {
	setup(t)

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("PUT", "/api/security/user/john/password", `{bad`, s)
	ctx.SetUserValue("user_name", "john")

	http_security.ChangeUserPasswordController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatal("expected 400")
	}
}

func TestChangeUserPasswordController_ForbiddenSelf(t *testing.T) {
	setup(t)

	s := login(t, "bob", security.RoleUser)
	ctx := newCtx("PUT", "/api/security/user/bob/password", `{"password":"x"}`, s)
	ctx.SetUserValue("user_name", "bob")

	http_security.ChangeUserPasswordController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatal("expected 403")
	}
}

func TestChangeUserPasswordController_UserNotFound(t *testing.T) {
	setup(t)

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("PUT", "/api/security/user/ghost/password", `{"password":"x"}`, s)
	ctx.SetUserValue("user_name", "ghost")

	http_security.ChangeUserPasswordController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNotFound {
		t.Fatal("expected 404")
	}
}

func TestChangeUserRoleController_OK(t *testing.T) {
	setup(t)

	_ = security.CreateBasicUser(&security.BasicUser{
		Username: "bob",
		Password: "x",
		Role:     security.RoleUser,
	})

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("PUT", "/api/security/user/bob/role", `{"role":"admin"}`, s)
	ctx.SetUserValue("user_name", "bob")

	http_security.ChangeUserRoleController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatal("expected 200")
	}

	u, err := security.GetBasicUserByUsername("bob")
	if err != nil {
		t.Fatal(err)
	}

	if u["role"] != string(security.RoleAdmin) {
		t.Fatal("role not updated")
	}
}

func TestChangeUserRoleController_Forbidden(t *testing.T) {
	setup(t)

	_ = security.CreateBasicUser(&security.BasicUser{
		Username: "bob",
		Password: "x",
		Role:     security.RoleUser,
	})

	s := login(t, "bob", security.RoleUser)
	ctx := newCtx("PUT", "/api/security/user/bob/role", `{"role":"admin"}`, s)
	ctx.SetUserValue("user_name", "bob")

	http_security.ChangeUserRoleController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatal("expected 403")
	}
}

func TestChangeUserRoleController_InvalidJSON(t *testing.T) {
	setup(t)

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("PUT", "/api/security/user/bob/role", `{bad`, s)
	ctx.SetUserValue("user_name", "bob")

	http_security.ChangeUserRoleController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatal("expected 400")
	}
}

func TestChangeUserRoleController_UserNotFound(t *testing.T) {
	setup(t)

	s := login(t, security.DefaultAdminUsername, security.RoleAdmin)
	ctx := newCtx("PUT", "/api/security/user/ghost/role", `{"role":"admin"}`, s)
	ctx.SetUserValue("user_name", "ghost")

	http_security.ChangeUserRoleController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNotFound {
		t.Fatal("expected 404")
	}
}
