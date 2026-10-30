package e2e

import (
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func startUserAuthTestServer(t *testing.T) (*fasthttp.Client, func()) {
	t.Helper()

	client, stop := startTestServerWithConfig(t, &configuration.Config{
		Store: configuration.StoreConfig{
			Folder: t.TempDir(),
			Shards: 8,
		},
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "user",
			},
		},
	})

	if err := security.InitAdminUserIfNotExists(); err != nil {
		stop()
		t.Fatalf("admin user creation failed: %v", err)
	}

	return client, stop
}

func TestLogoutEndsAdminSession(t *testing.T) {
	client, stop := startUserAuthTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodPost)
	req.SetRequestURI("http://test/api/security/login")
	req.Header.SetContentType("application/json")
	req.SetBodyString(`{"username":"admin","password":"admin"}`)
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("login failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200 on login, got %d", sc)
	}

	sessionCookie := fasthttp.AcquireCookie()
	defer fasthttp.ReleaseCookie(sessionCookie)
	sessionCookie.SetKey(security.SessionCookieName)
	if !resp.Header.Cookie(sessionCookie) || len(sessionCookie.Value()) == 0 {
		t.Fatalf("expected a session cookie on login")
	}
	sessionID := string(sessionCookie.Value())

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/api/security/me")
	req.Header.SetCookie(security.SessionCookieName, sessionID)
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("me failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200 on me before logout, got %d", sc)
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodPost)
	req.SetRequestURI("http://test/api/security/logout")
	req.Header.SetCookie(security.SessionCookieName, sessionID)
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("logout failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("expected 204 on logout, got %d", sc)
	}

	cleared := fasthttp.AcquireCookie()
	defer fasthttp.ReleaseCookie(cleared)
	cleared.SetKey(security.SessionCookieName)
	if !resp.Header.Cookie(cleared) {
		t.Fatalf("expected logout to reset the session cookie")
	}
	expire := cleared.Expire()
	if len(cleared.Value()) != 0 || expire.Equal(fasthttp.CookieExpireUnlimited) || !expire.Before(time.Now()) {
		t.Fatalf("expected an empty expired session cookie, got %s", cleared.String())
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/api/security/me")
	req.Header.SetCookie(security.SessionCookieName, sessionID)
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("me failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401 on me after logout, got %d", sc)
	}
}
