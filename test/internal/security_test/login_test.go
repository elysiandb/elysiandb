package security_test

import (
	"errors"
	"net"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func remoteSource(ip string) security.LoginSource {
	return security.LoginSource{IP: net.ParseIP(ip)}
}

func TestFailedLoginsLockTheClientForAWhile(t *testing.T) {
	setup(t)
	security.ResetLoginAttempts()
	defer security.ResetLoginAttempts()

	now := time.Now()
	security.SetLoginClock(func() time.Time { return now })
	defer security.SetLoginClock(nil)

	if err := security.CreateBasicUser(&security.BasicUser{Username: "ops", Password: "ops-password", Role: security.RoleAdmin}); err != nil {
		t.Fatal(err)
	}

	client := remoteSource("203.0.113.1")
	for i := 0; i < 5; i++ {
		if _, wait, ok := security.AuthenticateUserFrom("ops", "wrong", client); ok || wait != 0 {
			t.Fatalf("attempt %d: expected a plain failure", i)
		}
	}

	if _, wait, ok := security.AuthenticateUserFrom("ops", "ops-password", client); ok || wait <= 0 {
		t.Fatalf("expected the client to be locked, got ok=%v wait=%v", ok, wait)
	}

	if _, _, ok := security.AuthenticateUserFrom("ops", "ops-password", remoteSource("203.0.113.2")); !ok {
		t.Fatalf("expected another client to log in")
	}

	now = now.Add(2 * time.Second)
	if _, _, ok := security.AuthenticateUserFrom("ops", "ops-password", client); !ok {
		t.Fatalf("expected the lock to expire")
	}
}

func TestLockoutGrowsAndIsCapped(t *testing.T) {
	setup(t)
	security.ResetLoginAttempts()
	defer security.ResetLoginAttempts()

	now := time.Now()
	security.SetLoginClock(func() time.Time { return now })
	defer security.SetLoginClock(nil)

	client := remoteSource("203.0.113.3")
	var last time.Duration
	for i := 0; i < 30; i++ {
		_, _, _ = security.AuthenticateUserFrom("ghost", "wrong", client)
		_, wait, _ := security.AuthenticateUserFrom("ghost", "wrong", client)
		if wait > 15*time.Minute {
			t.Fatalf("expected the lock to be capped, got %v", wait)
		}
		if wait > 0 {
			last = wait
			now = now.Add(wait)
		}
	}

	if last != 15*time.Minute {
		t.Fatalf("expected the lock to reach its cap, got %v", last)
	}
}

func TestDefaultAdminPasswordOnlyFromTheLocalMachine(t *testing.T) {
	setup(t)
	security.ResetLoginAttempts()
	defer security.ResetLoginAttempts()

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatal(err)
	}

	if _, _, ok := security.AuthenticateUserFrom("admin", "admin", remoteSource("203.0.113.4")); ok {
		t.Fatalf("expected the default password to be refused remotely")
	}

	if _, _, ok := security.AuthenticateUserFrom("admin", "admin", security.LoginSource{IP: net.IPv4(127, 0, 0, 1), Local: true}); !ok {
		t.Fatalf("expected the default password to work locally")
	}

	if !security.DefaultAdminPasswordIsInUse() {
		t.Fatalf("expected the default password to be reported")
	}
}

func TestInitialAdminPasswordFromTheEnvironment(t *testing.T) {
	setup(t)
	t.Setenv(security.AdminPasswordEnvVar, "initial-admin-password")

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatal(err)
	}

	if _, ok := security.AuthenticateUser("admin", "initial-admin-password"); !ok {
		t.Fatalf("expected the password from the environment")
	}

	if security.DefaultAdminPasswordIsInUse() {
		t.Fatalf("expected the default password not to be set")
	}
}

func newRequestFrom(ip string, headers map[string]string) *fasthttp.RequestCtx {
	req := fasthttp.AcquireRequest()
	for name, value := range headers {
		req.Header.Add(name, value)
	}

	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, &net.TCPAddr{IP: net.ParseIP(ip)}, nil)

	return ctx
}

func TestLoginSourceFromRequest(t *testing.T) {
	setup(t)

	cases := []struct {
		name    string
		peer    string
		headers map[string]string
		proxies []string
		ip      string
		local   bool
	}{
		{name: "loopback", peer: "127.0.0.1", ip: "127.0.0.1", local: true},
		{name: "remote", peer: "203.0.113.5", ip: "203.0.113.5"},
		{name: "forwarded without trusted proxy", peer: "127.0.0.1", headers: map[string]string{"X-Forwarded-For": "127.0.0.1"}, ip: "127.0.0.1"},
		{name: "forwarded header ignored from untrusted peer", peer: "203.0.113.5", headers: map[string]string{"X-Forwarded-For": "127.0.0.1"}, ip: "203.0.113.5"},
		{name: "trusted proxy", peer: "10.0.0.2", headers: map[string]string{"X-Forwarded-For": "127.0.0.1, 198.51.100.7, 10.0.0.3"}, proxies: []string{"10.0.0.0/8"}, ip: "198.51.100.7"},
		{name: "trusted proxy for a local client", peer: "10.0.0.2", headers: map[string]string{"X-Forwarded-For": "127.0.0.1"}, proxies: []string{"10.0.0.2"}, ip: "127.0.0.1", local: true},
		{name: "invalid forwarded value", peer: "10.0.0.2", headers: map[string]string{"X-Forwarded-For": "not-an-ip"}, proxies: []string{"10.0.0.2"}, ip: "10.0.0.2"},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			setup(t)
			globals.GetConfig().Server.HTTP.TrustedProxies = tc.proxies

			source := security.LoginSourceFromRequest(newRequestFrom(tc.peer, tc.headers))
			if source.IP.String() != tc.ip || source.Local != tc.local {
				t.Fatalf("expected %s local=%v, got %s local=%v", tc.ip, tc.local, source.IP, source.Local)
			}
		})
	}
}

func TestUsernameAndPasswordRules(t *testing.T) {
	for _, username := range []string{"", ".", "..", "a/b", "with space", "tab\tname", string(make([]byte, 129))} {
		if !errors.Is(security.ValidateUsername(username), security.ErrInvalidUsername) {
			t.Fatalf("expected %q to be refused", username)
		}
	}

	for _, username := range []string{"admin", "john.doe@example.com", "o'neil", "élodie"} {
		if err := security.ValidateUsername(username); err != nil {
			t.Fatalf("expected %q to be accepted, got %v", username, err)
		}
	}

	if !errors.Is(security.ValidatePassword("1234567"), security.ErrPasswordTooShort) || security.ValidatePassword("12345678") != nil {
		t.Fatalf("unexpected password rule")
	}

	if security.ValidatePassword("éééééééé") != nil {
		t.Fatalf("expected characters to be counted, not bytes")
	}
}

func TestCrossOriginWriteDetection(t *testing.T) {
	setup(t)
	globals.GetConfig().Server.HTTP.CORS.AllowedOrigins = []string{"https://app.example"}

	cases := []struct {
		method  string
		headers map[string]string
		reject  bool
	}{
		{method: "POST"},
		{method: "POST", headers: map[string]string{"Sec-Fetch-Site": "same-origin"}},
		{method: "POST", headers: map[string]string{"Sec-Fetch-Site": "none"}},
		{method: "POST", headers: map[string]string{"Sec-Fetch-Site": "cross-site"}, reject: true},
		{method: "PUT", headers: map[string]string{"Sec-Fetch-Site": "same-site"}, reject: true},
		{method: "DELETE", headers: map[string]string{"Origin": "https://elsewhere.example"}, reject: true},
		{method: "POST", headers: map[string]string{"Origin": "null"}, reject: true},
		{method: "POST", headers: map[string]string{"Origin": "http://db.example:8089"}},
		{method: "POST", headers: map[string]string{"Origin": "https://app.example", "Sec-Fetch-Site": "cross-site"}},
		{method: "GET", headers: map[string]string{"Sec-Fetch-Site": "cross-site"}},
	}

	for _, tc := range cases {
		ctx := newRequestFrom("203.0.113.9", tc.headers)
		ctx.Request.Header.SetMethod(tc.method)
		ctx.Request.Header.SetHost("db.example:8089")

		if got := security.CrossOriginWriteIsRejected(ctx); got != tc.reject {
			t.Fatalf("%s %v: expected reject=%v", tc.method, tc.headers, tc.reject)
		}
	}
}
