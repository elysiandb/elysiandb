package e2e

import (
	"encoding/json"
	"net"
	"strings"
	"testing"
	"time"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
	"github.com/valyala/fasthttp/fasthttputil"
)

type apiServer struct {
	t       *testing.T
	ln      *fasthttputil.InmemoryListener
	client  *fasthttp.Client
	cookie  string
	headers map[string]string
}

type apiResponse struct {
	status  int
	body    []byte
	headers map[string]string
}

func (r apiResponse) header(name string) string {
	return r.headers[strings.ToLower(name)]
}

func (r apiResponse) json(t *testing.T) any {
	t.Helper()

	var out any
	if err := json.Unmarshal(r.body, &out); err != nil {
		t.Fatalf("invalid JSON body %q: %v", r.body, err)
	}

	return out
}

func (r apiResponse) object(t *testing.T) map[string]any {
	t.Helper()

	obj, ok := r.json(t).(map[string]any)
	if !ok {
		t.Fatalf("expected a JSON object, got %s", r.body)
	}

	return obj
}

func (r apiResponse) list(t *testing.T) []map[string]any {
	t.Helper()

	raw, ok := r.json(t).([]any)
	if !ok {
		t.Fatalf("expected a JSON array, got %s", r.body)
	}

	out := make([]map[string]any, 0, len(raw))
	for _, item := range raw {
		obj, ok := item.(map[string]any)
		if !ok {
			t.Fatalf("expected an array of objects, got %s", r.body)
		}
		out = append(out, obj)
	}

	return out
}

func (r apiResponse) ids(t *testing.T) []string {
	t.Helper()

	var ids []string
	for _, item := range r.list(t) {
		id, _ := item["id"].(string)
		ids = append(ids, id)
	}

	return ids
}

func defaultAPIConfig(folder string) *configuration.Config {
	return &configuration.Config{
		Store: configuration.StoreConfig{
			Folder: folder,
			Shards: 8,
		},
		Stats: configuration.StatsConfig{Enabled: true},
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: true,
				Mode:    "user",
				Token:   "test-token",
			},
		},
		Api: configuration.ApiConfig{
			Schema: configuration.ApiSchemaConfig{Enabled: true, Strict: true},
			Cache:  configuration.ApiCacheConfig{Enabled: true, CleanupIntervalSeconds: 10},
			Hooks:  configuration.HooksConfig{Enabled: true},
		},
	}
}

func startAPIServer(t *testing.T, configure func(cfg *configuration.Config)) *apiServer {
	t.Helper()

	cfg := defaultAPIConfig(t.TempDir())
	if configure != nil {
		configure(cfg)
	}

	globals.SetConfig(cfg)
	security.ResetLoginAttempts()
	storage.LoadDB()
	storage.LoadJsonDB()
	cache.InitCache(10 * time.Second)
	hook.InitHooks()

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatalf("cannot create the default admin: %v", err)
	}

	acl.InitACL()

	r := router.New()
	routing.RegisterRoutes(r)
	srv := &fasthttp.Server{Handler: security.Protect(r.Handler)}

	ln := fasthttputil.NewInmemoryListener()
	go func() { _ = srv.Serve(ln) }()

	t.Cleanup(func() {
		_ = ln.Close()
		_ = srv.Shutdown()
	})

	return &apiServer{
		t:       t,
		ln:      ln,
		client:  clientFrom(ln, "127.0.0.1"),
		headers: map[string]string{},
	}
}

func clientFrom(ln *fasthttputil.InmemoryListener, ip string) *fasthttp.Client {
	return &fasthttp.Client{
		Dial: func(addr string) (net.Conn, error) { return ln.DialWithLocalAddr(&net.TCPAddr{IP: net.ParseIP(ip)}) },
	}
}

func (s *apiServer) from(ip string) *apiServer {
	return &apiServer{t: s.t, ln: s.ln, client: clientFrom(s.ln, ip), headers: map[string]string{}}
}

func startAdminSession(t *testing.T, configure func(cfg *configuration.Config)) *apiServer {
	t.Helper()

	s := startAPIServer(t, configure)
	s.login(security.DefaultAdminUsername, security.DefaultAdminPassword)

	return s
}

func (s *apiServer) login(username, password string) {
	s.t.Helper()

	body, _ := json.Marshal(map[string]string{"username": username, "password": password})
	resp := s.do("POST", "/api/security/login", string(body))
	if resp.status != fasthttp.StatusOK {
		s.t.Fatalf("login as %s failed: %d %s", username, resp.status, resp.body)
	}

	for _, part := range strings.Split(resp.header("Set-Cookie"), ";") {
		part = strings.TrimSpace(part)
		if value, ok := strings.CutPrefix(part, security.SessionCookieName+"="); ok {
			s.cookie = value
		}
	}

	if s.cookie == "" {
		s.t.Fatalf("login as %s did not return a session cookie", username)
	}
}

func (s *apiServer) anonymous() *apiServer {
	return &apiServer{t: s.t, ln: s.ln, client: s.client, headers: map[string]string{}}
}

func (s *apiServer) do(method, path, body string, headers ...string) apiResponse {
	s.t.Helper()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(method)
	req.SetRequestURI("http://test" + path)
	if body != "" {
		req.SetBodyString(body)
	}

	if s.cookie != "" {
		req.Header.SetCookie(security.SessionCookieName, s.cookie)
	}

	for k, v := range s.headers {
		req.Header.Set(k, v)
	}

	for i := 0; i+1 < len(headers); i += 2 {
		req.Header.Set(headers[i], headers[i+1])
	}

	if err := s.client.Do(req, resp); err != nil {
		s.t.Fatalf("%s %s failed: %v", method, path, err)
	}

	out := apiResponse{
		status:  resp.StatusCode(),
		body:    append([]byte(nil), resp.Body()...),
		headers: map[string]string{},
	}

	for key, value := range resp.Header.All() {
		name := strings.ToLower(string(key))
		if existing, ok := out.headers[name]; ok {
			out.headers[name] = existing + "; " + string(value)
			continue
		}
		out.headers[name] = string(value)
	}

	return out
}

func (s *apiServer) mustDo(status int, method, path, body string, headers ...string) apiResponse {
	s.t.Helper()

	resp := s.do(method, path, body, headers...)
	if resp.status != status {
		s.t.Fatalf("%s %s: expected %d, got %d %s", method, path, status, resp.status, resp.body)
	}

	return resp
}
