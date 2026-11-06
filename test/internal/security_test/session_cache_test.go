package security_test

import (
	"encoding/base64"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func TestSessionsAreKeptInMemoryAndWrittenOnChange(t *testing.T) {
	dir := setupTempStore(t)
	seedUser(t, "user", security.RoleAdmin)

	s, err := security.CreateSession("user", security.RoleAdmin, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	if err := os.Chmod(dir, 0o500); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chmod(dir, 0o700) })

	for i := 0; i < 3; i++ {
		if found, err := security.GetSession(s.ID); err != nil || found == nil {
			t.Fatalf("reading a session needs no file access, got %v %v", found, err)
		}
	}

	_ = os.Chmod(dir, 0o700)
	if err := security.DeleteSession(s.ID); err != nil {
		t.Fatal(err)
	}

	if found, _ := security.GetSession(s.ID); found != nil {
		t.Fatalf("a deleted session must be gone")
	}

	raw, err := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	if err != nil || len(raw) == 0 {
		t.Fatalf("the change is written to the file: %q %v", raw, err)
	}
}

func TestDeleteAllSessionsEndsEverySession(t *testing.T) {
	setupTempStore(t)
	seedUser(t, "user", security.RoleAdmin)

	s, err := security.CreateSession("user", security.RoleAdmin, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	if err := security.DeleteAllSessions(); err != nil {
		t.Fatal(err)
	}

	if found, _ := security.GetSession(s.ID); found != nil {
		t.Fatalf("deleting every session ends every session")
	}
}

func TestActiveSessionCount(t *testing.T) {
	dir := setupTempStore(t)
	seedUser(t, "a", security.RoleAdmin)
	seedUser(t, "b", security.RoleAdmin)

	if n := security.ActiveSessionCount(); n != 0 {
		t.Fatalf("expected no session, got %d", n)
	}

	if _, err := security.CreateSession("a", security.RoleAdmin, time.Hour); err != nil {
		t.Fatal(err)
	}
	if _, err := security.CreateSession("b", security.RoleAdmin, time.Hour); err != nil {
		t.Fatal(err)
	}

	if n := security.ActiveSessionCount(); n != 2 {
		t.Fatalf("expected 2 sessions, got %d", n)
	}

	if _, err := security.CreateSession("a", security.RoleAdmin, time.Second); err != nil {
		t.Fatal(err)
	}
	time.Sleep(1100 * time.Millisecond)
	if n := security.ActiveSessionCount(); n != 2 {
		t.Fatalf("expired sessions are not counted, got %d", n)
	}
	_ = dir
}

func TestActorNamesTheCaller(t *testing.T) {
	setupTempStore(t)
	cfg := globals.GetConfig()

	ctx := &fasthttp.RequestCtx{}
	if got := security.Actor(ctx); got != "anonymous" {
		t.Fatalf("without authentication the actor is anonymous, got %q", got)
	}

	cfg.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "token", Token: "t"}
	if got := security.Actor(ctx); got != "token" {
		t.Fatalf("in token mode the actor is token, got %q", got)
	}

	cfg.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "basic"}
	req := fasthttp.AcquireRequest()
	req.Header.Set("Authorization", "Basic "+base64.StdEncoding.EncodeToString([]byte("alice:secret")))
	basic := &fasthttp.RequestCtx{}
	basic.Init(req, nil, nil)
	if got := security.Actor(basic); got != "alice" {
		t.Fatalf("in basic mode the actor is the username, got %q", got)
	}
	if got := security.Actor(ctx); got != "unknown" {
		t.Fatalf("without a basic header the actor is unknown, got %q", got)
	}

	ctx.SetUserValue("username", "bob")
	if got := security.Actor(ctx); got != "bob" {
		t.Fatalf("a session user wins, got %q", got)
	}
}
