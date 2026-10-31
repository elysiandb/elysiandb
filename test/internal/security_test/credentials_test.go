package security_test

import (
	"encoding/base64"
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func setAuthentication(enabled bool, mode, token string) {
	cfg := globals.GetConfig()
	cfg.Security.Authentication.Enabled = enabled
	cfg.Security.Authentication.Mode = mode
	cfg.Security.Authentication.Token = token
}

func TestTokenMatches(t *testing.T) {
	setup(t)
	setAuthentication(true, "token", "secret-token")

	if !security.TokenMatches("secret-token") {
		t.Fatalf("expected the token to match")
	}

	for _, wrong := range []string{"", "secret", "secret-token ", "secret-tokens", "SECRET-TOKEN"} {
		if security.TokenMatches(wrong) {
			t.Fatalf("expected %q not to match", wrong)
		}
	}

	setAuthentication(true, "token", "")
	if security.TokenMatches("") {
		t.Fatalf("an empty configured token must never match")
	}
}

func TestAuthenticateCredentials(t *testing.T) {
	setup(t)

	if err := security.CreateBasicUser(&security.BasicUser{Username: "ops", Password: "ops pass", Role: security.RoleAdmin}); err != nil {
		t.Fatal(err)
	}
	if err := security.CreateBasicUser(&security.BasicUser{Username: "alice", Password: "alice-pass", Role: security.RoleUser}); err != nil {
		t.Fatal(err)
	}

	setAuthentication(false, "", "")
	if !security.AuthenticateCredentials("") || security.CredentialsAreRequired() {
		t.Fatalf("expected no credentials to be required with authentication disabled")
	}

	setAuthentication(true, "token", "tcp-token")
	if !security.CredentialsAreRequired() || !security.AuthenticateCredentials(" tcp-token ") || security.AuthenticateCredentials("ops ops pass") {
		t.Fatalf("unexpected token mode result")
	}

	setAuthentication(true, "basic", "")
	if !security.AuthenticateCredentials("alice alice-pass") || !security.AuthenticateCredentials("ops ops pass") {
		t.Fatalf("expected any valid user in basic mode")
	}
	if security.AuthenticateCredentials("alice wrong") || security.AuthenticateCredentials("alice") || security.AuthenticateCredentials("ghost x") {
		t.Fatalf("expected invalid credentials to be refused in basic mode")
	}

	setAuthentication(true, "user", "")
	if !security.AuthenticateCredentials("ops ops pass") {
		t.Fatalf("expected an admin to authenticate in user mode")
	}
	if security.AuthenticateCredentials("alice alice-pass") {
		t.Fatalf("expected a regular user to be refused in user mode")
	}
}

func TestCurrentUserIsAdminWithoutAuthentication(t *testing.T) {
	setup(t)
	setAuthentication(false, "", "")

	var ctx fasthttp.RequestCtx
	if !security.CurrentUserIsAdmin(&ctx) {
		t.Fatalf("expected every request to be allowed with authentication disabled")
	}
}

func TestCurrentUserIsAdminInBasicMode(t *testing.T) {
	setup(t)
	setAuthentication(true, "basic", "")

	if err := security.CreateBasicUser(&security.BasicUser{Username: "john", Password: "secret", Role: security.RoleUser}); err != nil {
		t.Fatal(err)
	}

	newCtx := func(header string) *fasthttp.RequestCtx {
		req := fasthttp.AcquireRequest()
		if header != "" {
			req.Header.Set("Authorization", header)
		}
		ctx := &fasthttp.RequestCtx{}
		ctx.Init(req, nil, nil)
		return ctx
	}

	if !security.CurrentUserIsAdmin(newCtx("Basic " + base64.StdEncoding.EncodeToString([]byte("john:secret")))) {
		t.Fatalf("expected valid basic credentials to be accepted")
	}

	if security.CurrentUserIsAdmin(newCtx("")) || security.CurrentUserIsAdmin(newCtx("Basic "+base64.StdEncoding.EncodeToString([]byte("john:wrong")))) {
		t.Fatalf("expected missing or wrong credentials to be refused")
	}
}

func TestConcurrentSessionsAreAllKept(t *testing.T) {
	setup(t)

	const count = 40
	ids := make([]string, count)

	var wg sync.WaitGroup
	for i := 0; i < count; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			s, err := security.CreateSession(fmt.Sprintf("user%d", i), security.RoleAdmin, time.Hour)
			if err != nil {
				t.Errorf("CreateSession: %v", err)
				return
			}
			ids[i] = s.ID
		}(i)
	}
	wg.Wait()

	for i, id := range ids {
		s, err := security.GetSession(id)
		if err != nil || s == nil {
			t.Fatalf("session %d was lost (err=%v)", i, err)
		}
	}

	var readers sync.WaitGroup
	for i := 0; i < count; i++ {
		readers.Add(2)
		go func(id string) {
			defer readers.Done()
			if s, err := security.GetSession(id); err != nil || s == nil {
				t.Errorf("concurrent read failed for %s: %v", id, err)
			}
		}(ids[i])
		go func(i int) {
			defer readers.Done()
			if _, err := security.CreateSession(fmt.Sprintf("extra%d", i), security.RoleUser, time.Hour); err != nil {
				t.Errorf("CreateSession: %v", err)
			}
		}(i)
	}
	readers.Wait()

	for i := 0; i < count/2; i++ {
		if err := security.DeleteSession(ids[i]); err != nil {
			t.Fatal(err)
		}
	}

	for i, id := range ids {
		s, _ := security.GetSession(id)
		if (i < count/2) != (s == nil) {
			t.Fatalf("session %d: unexpected state after deleting the first half", i)
		}
	}
}

func TestDeleteBasicUserProtectsDefaultAndLastAdmin(t *testing.T) {
	setup(t)

	for _, u := range []*security.BasicUser{
		{Username: security.DefaultAdminUsername, Password: "x", Role: security.RoleAdmin},
		{Username: "ops", Password: "x", Role: security.RoleAdmin},
		{Username: "alice", Password: "x", Role: security.RoleUser},
	} {
		if err := security.CreateBasicUser(u); err != nil {
			t.Fatal(err)
		}
	}

	if err := security.DeleteBasicUser(security.DefaultAdminUsername); !errors.Is(err, security.ErrDefaultAdminProtected) {
		t.Fatalf("expected the default admin to be protected, got %v", err)
	}

	if err := security.DeleteBasicUser("alice"); err != nil {
		t.Fatalf("expected a regular user to be deleted, got %v", err)
	}

	if err := security.ChangeUserRole(security.DefaultAdminUsername, "user"); err != nil {
		t.Fatalf("expected a demotion to succeed while another admin exists, got %v", err)
	}

	if err := security.DeleteBasicUser("ops"); !errors.Is(err, security.ErrLastAdmin) {
		t.Fatalf("expected the last admin to be protected, got %v", err)
	}

	if err := security.ChangeUserRole("ops", "user"); !errors.Is(err, security.ErrLastAdmin) {
		t.Fatalf("expected the last admin to keep its role, got %v", err)
	}

	if err := security.ChangeUserRole("ops", "root"); !errors.Is(err, security.ErrInvalidRole) {
		t.Fatalf("expected an invalid role to be refused, got %v", err)
	}

	if err := security.ChangeUserRole("ghost", "user"); !errors.Is(err, security.ErrUserNotFound) || err.Error() != "user 'ghost' not found" {
		t.Fatalf("expected a not found error, got %v", err)
	}
}
