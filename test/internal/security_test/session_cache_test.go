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

func TestSessionsFileChangedOutsideIsReloaded(t *testing.T) {
	dir := setupTempStore(t)
	seedUser(t, "user", security.RoleAdmin)

	s, err := security.CreateSession("user", security.RoleAdmin, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	if found, err := security.GetSession(s.ID); err != nil || found == nil {
		t.Fatalf("expected the session, got %v %v", found, err)
	}

	time.Sleep(10 * time.Millisecond)
	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{}})

	if found, _ := security.GetSession(s.ID); found != nil {
		t.Fatalf("a session removed from the file by another process must be gone")
	}
}

func TestSessionsFileRemovedOutsideIsReloaded(t *testing.T) {
	dir := setupTempStore(t)
	seedUser(t, "user", security.RoleAdmin)

	s, err := security.CreateSession("user", security.RoleAdmin, time.Hour)
	if err != nil {
		t.Fatal(err)
	}

	if err := os.Remove(filepath.Join(dir, security.SessionsFilename)); err != nil {
		t.Fatal(err)
	}

	if found, _ := security.GetSession(s.ID); found != nil {
		t.Fatalf("a deleted sessions file ends every session")
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

	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{{TokenHash: "x", Username: "a", ExpiresAt: time.Now().Add(-time.Minute).Unix()}}})
	if n := security.ActiveSessionCount(); n != 0 {
		t.Fatalf("expired sessions are not counted, got %d", n)
	}
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
