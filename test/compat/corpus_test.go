package compat_test

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	tcprouting "github.com/taymour/elysiandb/internal/transport/tcp/tcp_routing"
	"github.com/valyala/fasthttp"
	"github.com/valyala/fasthttp/fasthttputil"
)

const goldenPath = "testdata/golden.json"

type goldenEntry struct {
	Request     fixtureRequest `json:"request"`
	Status      int            `json:"status"`
	ContentType string         `json:"content_type,omitempty"`
	Body        string         `json:"body"`
}

type corpusServer struct {
	t       *testing.T
	client  *fasthttp.Client
	cookies map[string]string
	tcp     *tcprouting.Session
	conn    net.Conn
	txID    string
}

var (
	uuidPattern = regexp.MustCompile(`[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`)
	txPattern   = regexp.MustCompile(`\d{14}\.\d{9}`)
)

func corpusConfig(folder string) *configuration.Config {
	return &configuration.Config{
		Store: configuration.StoreConfig{Folder: folder, Shards: 8, FlushIntervalSeconds: 0},
		Server: configuration.ServersConfig{
			HTTP: configuration.ServerConfig{Enabled: true, Host: "127.0.0.1", Port: 1},
			TCP:  configuration.ServerConfig{Enabled: true, Host: "127.0.0.1", Port: 1},
		},
		Security: configuration.SecurityConfig{Authentication: configuration.AuthenticationConfig{Enabled: true, Mode: "user"}},
		Api: configuration.ApiConfig{
			Schema: configuration.ApiSchemaConfig{Enabled: true, Strict: true},
			Cache:  configuration.ApiCacheConfig{Enabled: true, CleanupIntervalSeconds: 10},
			Hooks:  configuration.HooksConfig{Enabled: true},
			Index:  configuration.ApiIndexConfig{Workers: 1},
		},
		Log: configuration.LogConfig{Level: "error", Format: "text"},
	}
}

func startCorpusServer(t *testing.T) *corpusServer {
	t.Helper()

	cfg := corpusConfig(t.TempDir())
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
	srv := &fasthttp.Server{Handler: boot.NewHTTPHandler(r)}

	ln := fasthttputil.NewInmemoryListener()
	go func() { _ = srv.Serve(ln) }()

	t.Cleanup(func() {
		_ = ln.Close()
		_ = srv.Shutdown()
	})

	s := &corpusServer{
		t: t,
		client: &fasthttp.Client{
			Dial: func(string) (net.Conn, error) {
				return ln.DialWithLocalAddr(&net.TCPAddr{IP: net.ParseIP("127.0.0.1")})
			},
		},
		cookies: map[string]string{},
		tcp:     tcprouting.NewSession(),
	}

	local, remote := net.Pipe()
	s.conn = loopbackConn{Conn: local}
	t.Cleanup(func() {
		_ = local.Close()
		_ = remote.Close()
	})

	s.login("admin", security.DefaultAdminUsername, security.DefaultAdminPassword)
	auth := tcprouting.RouteLine([]byte("AUTH "+security.DefaultAdminUsername+" "+security.DefaultAdminPassword+"\n"), s.conn, s.tcp)
	if string(auth) != "OK" {
		t.Fatalf("tcp auth failed: %s", auth)
	}

	return s
}

type loopbackConn struct {
	net.Conn
}

func (loopbackConn) RemoteAddr() net.Addr {
	return &net.TCPAddr{IP: net.ParseIP("127.0.0.1"), Port: 40000}
}

func (s *corpusServer) login(as, username, password string) {
	s.t.Helper()

	body, _ := json.Marshal(map[string]string{"username": username, "password": password})
	status, _, _, headers := s.http("POST", "/api/security/login", string(body), "")
	if status != fasthttp.StatusOK {
		s.t.Fatalf("login as %s failed: %d", username, status)
	}

	for _, part := range strings.Split(headers, ";") {
		if value, ok := strings.CutPrefix(strings.TrimSpace(part), security.SessionCookieName+"="); ok {
			s.cookies[as] = value
		}
	}
}

func (s *corpusServer) http(method, path, body, as string) (int, string, []byte, string) {
	s.t.Helper()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(method)
	req.SetRequestURI("http://corpus" + path)
	if body != "" {
		req.SetBodyString(body)
	}

	if as == "" {
		as = "admin"
	}

	if cookie := s.cookies[as]; cookie != "" {
		req.Header.SetCookie(security.SessionCookieName, cookie)
	}

	if err := s.client.DoTimeout(req, resp, 30*time.Second); err != nil {
		s.t.Fatalf("%s %s failed: %v", method, path, err)
	}

	return resp.StatusCode(), string(resp.Header.ContentType()), append([]byte(nil), resp.Body()...), string(resp.Header.Peek("Set-Cookie"))
}

func (s *corpusServer) run(r fixtureRequest) goldenEntry {
	s.t.Helper()

	switch r.Method {
	case "LOGIN":
		session, err := security.CreateSession(r.Path, security.RoleUser, time.Hour)
		if err != nil {
			s.t.Fatalf("cannot open a session for %s: %v", r.Path, err)
		}
		s.cookies[r.As] = session.ID
		return goldenEntry{Request: r, Status: fasthttp.StatusOK}
	case "TCP":
		out := tcprouting.RouteLine([]byte(r.Path+"\n"), s.conn, s.tcp)
		return goldenEntry{Request: r, Body: normalizeBody(r, out)}
	}

	path := strings.ReplaceAll(r.Path, "{tx}", s.txID)
	status, contentType, body, _ := s.http(r.Method, path, r.Body, r.As)

	if r.Path == "/api/tx/begin" {
		var begin struct {
			ID string `json:"transaction_id"`
		}
		_ = json.Unmarshal(body, &begin)
		s.txID = begin.ID
	}

	return goldenEntry{Request: r, Status: status, ContentType: contentType, Body: normalizeBody(r, body)}
}

func normalizeBody(r fixtureRequest, body []byte) string {
	if r.Unordered {
		body = sortJSONArray(body)
	}

	out := uuidPattern.ReplaceAll(body, []byte("<uuid>"))
	out = txPattern.ReplaceAll(out, []byte("<tx>"))

	return string(out)
}

func sortJSONArray(body []byte) []byte {
	var items []json.RawMessage
	if err := json.Unmarshal(body, &items); err != nil {
		lines := strings.Split(string(body), "\n")
		sort.Strings(lines)
		return []byte(strings.Join(lines, "\n"))
	}

	sort.Slice(items, func(i, j int) bool { return bytes.Compare(items[i], items[j]) < 0 })
	sorted, _ := json.Marshal(items)

	return sorted
}

func corpusRequests() []fixtureRequest {
	reqs := []fixtureRequest{}
	reqs = append(reqs, fixtureLoadRequests()...)
	reqs = append(reqs, hookRequests()...)
	reqs = append(reqs, readRequests()...)
	reqs = append(reqs, writeRequests()...)
	reqs = append(reqs, transactionRequests()...)
	reqs = append(reqs, kvRequests()...)
	reqs = append(reqs, ownershipRequests()...)

	return reqs
}

func runCorpus(t *testing.T) []goldenEntry {
	t.Helper()

	s := startCorpusServer(t)
	entries := []goldenEntry{}
	for _, r := range corpusRequests() {
		entries = append(entries, s.run(r))
	}

	return entries
}

func TestGoldenCorpus(t *testing.T) {
	entries := runCorpus(t)

	if os.Getenv("ELYSIAN_RECORD_GOLDEN") == "1" {
		data, err := json.MarshalIndent(entries, "", "  ")
		if err != nil {
			t.Fatal(err)
		}

		if err := os.MkdirAll(filepath.Dir(goldenPath), 0o755); err != nil {
			t.Fatal(err)
		}

		if err := os.WriteFile(goldenPath, append(data, '\n'), 0o644); err != nil {
			t.Fatal(err)
		}

		t.Logf("recorded %d entries", len(entries))
		return
	}

	raw, err := os.ReadFile(goldenPath)
	if err != nil {
		t.Fatalf("cannot read %s: %v", goldenPath, err)
	}

	var golden []goldenEntry
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("cannot parse %s: %v", goldenPath, err)
	}

	if len(golden) != len(entries) {
		t.Fatalf("corpus has %d requests, golden file has %d", len(entries), len(golden))
	}

	failures := 0
	for i, want := range golden {
		got := entries[i]
		if intentionalChange(want.Request) {
			continue
		}

		if got.Status != want.Status || got.ContentType != want.ContentType || got.Body != want.Body {
			failures++
			if failures <= 25 {
				t.Errorf("#%d %s %s\nwant %d %q %s\ngot  %d %q %s", i, want.Request.Method, want.Request.Path, want.Status, want.ContentType, truncate(want.Body), got.Status, got.ContentType, truncate(got.Body))
			}
		}
	}

	if failures > 0 {
		t.Fatalf("%d of %d responses differ from the golden corpus", failures, len(golden))
	}
}

func truncate(s string) string {
	if len(s) > 600 {
		return s[:600] + fmt.Sprintf("... (%d bytes)", len(s))
	}

	return s
}

func intentionalChange(r fixtureRequest) bool {
	return false
}
