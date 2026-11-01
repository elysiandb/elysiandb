package security_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
)

func setupTempStore(t *testing.T) string {
	t.Helper()

	dir := t.TempDir()

	cfg := &configuration.Config{}
	cfg.Store.Folder = dir
	cfg.Store.Shards = 4
	cfg.Security.Authentication.Enabled = false

	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()

	return dir
}

func seedUser(t *testing.T, username string, role security.Role) {
	t.Helper()

	if err := security.InitBasicUsersStorage(); err != nil {
		t.Fatal(err)
	}

	user := &security.BasicHashedUser{Username: username, Password: "hash-of-" + username, Role: role}
	if err := user.Save(); err != nil {
		t.Fatal(err)
	}
}

func enableUserMode() {
	cfg := globals.GetConfig()
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
}

func writeSessions(t *testing.T, dir string, sf security.SessionsFile) {
	t.Helper()
	b, _ := json.Marshal(sf)
	_ = os.WriteFile(filepath.Join(dir, security.SessionsFilename), b, 0o644)
}

func TestGetCurrentUsername(t *testing.T) {
	ctx := &fasthttp.RequestCtx{}
	if security.GetCurrentUsername(ctx) != "" {
		t.Fatalf("expected empty username")
	}

	ctx.SetUserValue("username", "alice")
	if security.GetCurrentUsername(ctx) != "alice" {
		t.Fatalf("expected alice")
	}
}

func TestCreateSession(t *testing.T) {
	dir := setupTempStore(t)
	seedUser(t, "user1", security.RoleAdmin)

	s, err := security.CreateSession("user1", security.RoleAdmin, time.Second)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if s.Username != "user1" || s.Role != security.RoleAdmin {
		t.Fatalf("invalid session data")
	}
	if s.ID == "" {
		t.Fatalf("session id not generated")
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 1 {
		t.Fatalf("session not saved")
	}
}

func TestCreateSession_CleansExpired(t *testing.T) {
	dir := setupTempStore(t)

	expired := security.Session{
		ID:        "old",
		Username:  "u",
		Role:      security.RoleUser,
		ExpiresAt: time.Now().Unix() - 10,
	}
	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{expired}})
	seedUser(t, "new", security.RoleUser)

	_, err := security.CreateSession("new", security.RoleUser, time.Second)
	if err != nil {
		t.Fatal(err)
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 1 {
		t.Fatalf("expired sessions not cleaned")
	}
}

func TestGetSession(t *testing.T) {
	setupTempStore(t)
	seedUser(t, "user", security.RoleUser)

	s, err := security.CreateSession("user", security.RoleUser, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	res, err := security.GetSession(s.ID)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res == nil || res.Username != "user" {
		t.Fatalf("session not returned")
	}
}

func TestGetSessionExpired(t *testing.T) {
	dir := setupTempStore(t)

	s := security.Session{
		ID:        "expired",
		Username:  "user",
		Role:      security.RoleUser,
		ExpiresAt: time.Now().Unix() - 10,
	}
	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{s}})

	res, err := security.GetSession("expired")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res != nil {
		t.Fatalf("expired session should be nil")
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 0 {
		t.Fatalf("expired session not cleaned")
	}
}

func TestDeleteSession(t *testing.T) {
	dir := setupTempStore(t)
	seedUser(t, "user", security.RoleUser)

	s, err := security.CreateSession("user", security.RoleUser, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	err = security.DeleteSession(s.ID)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 0 {
		t.Fatalf("session not deleted")
	}
}

func TestDeleteSessionNonExisting(t *testing.T) {
	dir := setupTempStore(t)

	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{}})

	err := security.DeleteSession("none")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 0 {
		t.Fatalf("file modified incorrectly")
	}
}

func TestCurrentSession(t *testing.T) {
	setupTempStore(t)

	seedUser(t, "me", security.RoleUser)
	s, _ := security.CreateSession("me", security.RoleUser, time.Second)
	req := fasthttp.AcquireRequest()
	req.Header.SetCookie(security.SessionCookieName, s.ID)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	cs, err := security.CurrentSession(&ctx)
	if err != nil || cs == nil || cs.Username != "me" {
		t.Fatalf("current session not resolved")
	}
}

func TestCurrentUserIsAdmin(t *testing.T) {
	setupTempStore(t)

	seedUser(t, "admin", security.RoleAdmin)
	s, _ := security.CreateSession("admin", security.RoleAdmin, time.Second)
	req := fasthttp.AcquireRequest()
	req.Header.SetCookie(security.SessionCookieName, s.ID)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if !security.CurrentUserIsAdmin(&ctx) {
		t.Fatalf("expected admin")
	}
}

func TestCurrentUserIsAdmin_False(t *testing.T) {
	setupTempStore(t)
	enableUserMode()

	seedUser(t, "user", security.RoleUser)
	s, _ := security.CreateSession("user", security.RoleUser, time.Second)
	req := fasthttp.AcquireRequest()
	req.Header.SetCookie(security.SessionCookieName, s.ID)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if security.CurrentUserIsAdmin(&ctx) {
		t.Fatalf("expected non admin")
	}
}

func setupTokenMode(t *testing.T) {
	setupTempStore(t)
	cfg := globals.GetConfig()
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "token"
	cfg.Security.Authentication.Token = "secret-token"
}

func TestCurrentUserIsAdmin_TokenMode_ValidToken(t *testing.T) {
	setupTokenMode(t)

	req := fasthttp.AcquireRequest()
	req.Header.Set("Authorization", "Bearer secret-token")

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if !security.CurrentUserIsAdmin(&ctx) {
		t.Fatalf("expected admin")
	}
}

func TestCurrentUserIsAdmin_TokenMode_InvalidToken(t *testing.T) {
	setupTokenMode(t)

	req := fasthttp.AcquireRequest()
	req.Header.Set("Authorization", "Bearer wrong-token")

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if security.CurrentUserIsAdmin(&ctx) {
		t.Fatalf("expected non admin")
	}
}

func TestCurrentUserIsAdmin_TokenMode_SessionCookieIsNotEnough(t *testing.T) {
	setupTokenMode(t)

	seedUser(t, "admin", security.RoleAdmin)
	s, _ := security.CreateSession("admin", security.RoleAdmin, time.Second)
	req := fasthttp.AcquireRequest()
	req.Header.SetCookie(security.SessionCookieName, s.ID)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	if security.CurrentUserIsAdmin(&ctx) {
		t.Fatalf("expected non admin")
	}
}

func TestCurrentUserCanManageUser_Admin(t *testing.T) {
	setupTempStore(t)

	seedUser(t, "admin", security.RoleAdmin)
	s, _ := security.CreateSession("admin", security.RoleAdmin, time.Second)
	req := fasthttp.AcquireRequest()
	req.Header.SetCookie(security.SessionCookieName, s.ID)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	ok, err := security.CurrentUserCanManageUser(&ctx, "someone")
	if err != nil || !ok {
		t.Fatalf("admin should manage anyone")
	}
}

func TestCurrentUserCanManageUser_Self(t *testing.T) {
	setupTempStore(t)
	enableUserMode()

	seedUser(t, "bob", security.RoleUser)
	s, _ := security.CreateSession("bob", security.RoleUser, time.Second)
	req := fasthttp.AcquireRequest()
	req.Header.SetCookie(security.SessionCookieName, s.ID)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	ok, err := security.CurrentUserCanManageUser(&ctx, "bob")
	if err != nil || ok {
		t.Fatalf("user should not manage self")
	}
}

func TestCurrentUserCanManageUser_Other(t *testing.T) {
	setupTempStore(t)

	seedUser(t, "bob", security.RoleUser)
	s, _ := security.CreateSession("bob", security.RoleUser, time.Second)
	req := fasthttp.AcquireRequest()
	req.Header.SetCookie(security.SessionCookieName, s.ID)

	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)

	ok, err := security.CurrentUserCanManageUser(&ctx, "alice")
	if err != nil || !ok {
		t.Fatalf("user should manage other user")
	}
}

func TestSessionsFileHoldsNoUsableID(t *testing.T) {
	dir := setupTempStore(t)
	seedUser(t, "admin", security.RoleAdmin)

	s, err := security.CreateSession("admin", security.RoleAdmin, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	path := filepath.Join(dir, security.SessionsFilename)
	data, _ := os.ReadFile(path)
	if strings.Contains(string(data), s.ID) {
		t.Fatalf("expected the session ID not to be stored, got %s", data)
	}

	info, err := os.Stat(path)
	if err != nil || info.Mode().Perm() != 0o600 {
		t.Fatalf("expected the sessions file to be private, got %v %v", info.Mode().Perm(), err)
	}
}

func TestSessionEndsWhenTheAccountChanges(t *testing.T) {
	cases := map[string]func(t *testing.T){
		"deleted": func(t *testing.T) {
			seedUser(t, "other", security.RoleAdmin)
			if err := security.DeleteBasicUser("bob"); err != nil {
				t.Fatal(err)
			}
		},
		"password changed": func(t *testing.T) {
			if err := security.ChangeUserPassword("bob", "another-password"); err != nil {
				t.Fatal(err)
			}
		},
		"demoted": func(t *testing.T) {
			seedUser(t, "other", security.RoleAdmin)
			if err := security.ChangeUserRole("bob", "user"); err != nil {
				t.Fatal(err)
			}
		},
		"recreated": func(t *testing.T) {
			seedUser(t, "bob", security.RoleAdmin)
		},
	}

	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			setupTempStore(t)
			if err := security.CreateBasicUser(&security.BasicUser{Username: "bob", Password: "bob-password", Role: security.RoleAdmin}); err != nil {
				t.Fatal(err)
			}

			s, err := security.CreateSession("bob", security.RoleAdmin, time.Hour)
			if err != nil {
				t.Fatal(err)
			}

			change(t)

			if got, _ := security.GetSession(s.ID); got != nil {
				t.Fatalf("expected the session to end, got %+v", got)
			}
		})
	}
}

func TestSessionFollowsTheCurrentRole(t *testing.T) {
	setupTempStore(t)
	seedUser(t, "bob", security.RoleUser)

	s, err := security.CreateSession("bob", security.RoleAdmin, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	got, _ := security.GetSession(s.ID)
	if got == nil || got.Role != security.RoleUser {
		t.Fatalf("expected the stored role to apply, got %+v", got)
	}
}

func TestPasswordChangeCanKeepTheCurrentSession(t *testing.T) {
	setupTempStore(t)
	if err := security.CreateBasicUser(&security.BasicUser{Username: "bob", Password: "bob-password", Role: security.RoleAdmin}); err != nil {
		t.Fatal(err)
	}

	current, _ := security.CreateSession("bob", security.RoleAdmin, time.Hour)
	other, _ := security.CreateSession("bob", security.RoleAdmin, time.Hour)

	if err := security.ChangeUserPasswordKeepingSession("bob", "another-password", current.ID); err != nil {
		t.Fatal(err)
	}

	if got, _ := security.GetSession(current.ID); got == nil {
		t.Fatalf("expected the current session to stay open")
	}

	if got, _ := security.GetSession(other.ID); got != nil {
		t.Fatalf("expected the other session to end")
	}
}

func TestCreateSessionRequiresAnExistingUser(t *testing.T) {
	setupTempStore(t)

	if _, err := security.CreateSession("ghost", security.RoleAdmin, time.Hour); err == nil {
		t.Fatalf("expected an error for an unknown user")
	}
}

func TestProtectSessionsFile(t *testing.T) {
	dir := setupTempStore(t)
	path := filepath.Join(dir, security.SessionsFilename)
	if err := os.WriteFile(path, []byte(`{"sessions":[]}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(path, 0o644); err != nil {
		t.Fatal(err)
	}

	security.ProtectSessionsFile()

	info, err := os.Stat(path)
	if err != nil || info.Mode().Perm() != 0o600 {
		t.Fatalf("expected the sessions file to be private, got %v %v", info.Mode().Perm(), err)
	}

	security.ProtectSessionsFile()
}
