package engines

import (
	"encoding/json"
	"fmt"
	"net"
	"regexp"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/test/mongotest"
	"github.com/valyala/fasthttp"
	"github.com/valyala/fasthttp/fasthttputil"
)

var (
	uuidPattern  = regexp.MustCompile(`[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}`)
	txIdPattern  = regexp.MustCompile(`\d{14}\.\d{9}`)
	enginesToRun = []string{engine.EngineInternal, engine.EngineMongoDB}
)

type exchange struct {
	Method string
	Path   string
	Status int
	Body   any
}

type response struct {
	Status int
	Header map[string]string
	Raw    []byte
	JSON   any
}

type session struct {
	t       *testing.T
	engine  string
	client  *fasthttp.Client
	cookie  string
	log     []exchange
	mu      sync.Mutex
	panics  []string
	closeFn func()
}

type scenario func(s *session)

func runOnEngines(t *testing.T, configure func(*configuration.Config), run scenario) {
	t.Helper()

	logs := map[string][]exchange{}
	for _, name := range enginesToRun {
		s := startEngine(t, name, configure)
		run(s)
		s.stop()
		logs[name] = s.log
	}

	compareLogs(t, logs[engine.EngineInternal], logs[engine.EngineMongoDB])
}

func startEngine(t *testing.T, name string, configure func(*configuration.Config)) *session {
	t.Helper()

	cfg := &configuration.Config{
		Store:  configuration.StoreConfig{Folder: t.TempDir(), Shards: 8},
		Engine: configuration.EngineConfig{Name: name},
		Stats:  configuration.StatsConfig{Enabled: true},
		Api: configuration.ApiConfig{
			Schema: configuration.ApiSchemaConfig{Enabled: true},
			Hooks:  configuration.HooksConfig{Enabled: true},
		},
	}
	if configure != nil {
		configure(cfg)
	}

	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()
	stat.Init()

	if name == engine.EngineMongoDB {
		mongotest.Use(t)
	}

	if cfg.Api.Cache.Enabled {
		cache.InitCache(time.Minute)
	}

	if security.UserAuthenticationIsEnabled() {
		if err := security.InitAdminUserIfNotExists(); err != nil {
			t.Fatalf("init admin: %v", err)
		}
	}
	acl.InitACL()
	hook.InitHooks()
	security.SetCurrentUsername("")

	s := &session{t: t, engine: name}

	r := router.New()
	routing.RegisterRoutes(r)
	srv := &fasthttp.Server{Handler: s.recoverPanics(r.Handler)}
	ln := fasthttputil.NewInmemoryListener()
	go func() { _ = srv.Serve(ln) }()

	s.client = &fasthttp.Client{Dial: func(string) (net.Conn, error) { return ln.Dial() }}
	s.closeFn = func() {
		_ = ln.Close()
		_ = srv.Shutdown()
	}

	return s
}

func (s *session) recoverPanics(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		defer func() {
			if r := recover(); r != nil {
				s.mu.Lock()
				s.panics = append(s.panics, fmt.Sprintf("%s %s: %v", ctx.Method(), ctx.RequestURI(), r))
				s.mu.Unlock()
				ctx.SetStatusCode(599)
			}
		}()

		next(ctx)
	}
}

func (s *session) stop() {
	s.t.Helper()
	s.closeFn()

	if len(s.panics) > 0 {
		s.t.Fatalf("[%s] handler panicked: %s", s.engine, strings.Join(s.panics, "; "))
	}

	if s.engine != engine.EngineMongoDB {
		return
	}

	if keys := storage.GetByWildcardKey("api:*"); len(keys) > 0 {
		names := make([]string, 0, len(keys))
		for k := range keys {
			names = append(names, k)
		}
		sort.Strings(names)
		s.t.Fatalf("[mongodb] API metadata leaked into the internal KV store: %v", names)
	}

	if n := storage.DeleteJsonByPrefix(""); n > 0 {
		s.t.Fatalf("[mongodb] %d documents leaked into the internal JSON store", n)
	}
}

func (s *session) loginAs(username string, role security.Role) {
	s.t.Helper()

	sess, err := security.CreateSession(username, role, time.Hour)
	if err != nil {
		s.t.Fatalf("create session: %v", err)
	}

	s.cookie = sess.ID
}

func (s *session) do(method, path string, body any) response {
	s.t.Helper()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(method)
	req.SetRequestURI("http://elysian" + path)
	if s.cookie != "" {
		req.Header.SetCookie(security.SessionCookieName, s.cookie)
	}

	switch b := body.(type) {
	case nil:
	case string:
		req.SetBodyString(b)
	default:
		raw, err := json.Marshal(b)
		if err != nil {
			s.t.Fatalf("marshal request: %v", err)
		}
		req.Header.SetContentType("application/json")
		req.SetBody(raw)
	}

	if err := s.client.DoTimeout(req, resp, 30*time.Second); err != nil {
		s.t.Fatalf("[%s] %s %s: %v", s.engine, method, path, err)
	}

	out := response{
		Status: resp.StatusCode(),
		Header: map[string]string{},
		Raw:    append([]byte(nil), resp.Body()...),
	}
	for _, h := range []string{"X-Elysian-Cache", "Content-Type"} {
		out.Header[h] = string(resp.Header.Peek(h))
	}

	if len(out.Raw) > 0 {
		var parsed any
		if json.Unmarshal(out.Raw, &parsed) == nil {
			out.JSON = parsed
		}
	}

	return out
}

func (s *session) call(method, path string, body any) response {
	s.t.Helper()

	resp := s.do(method, path, body)

	recorded := any(string(resp.Raw))
	if resp.JSON != nil {
		recorded = resp.JSON
	}

	s.log = append(s.log, exchange{Method: method, Path: path, Status: resp.Status, Body: recorded})

	return resp
}

func (s *session) callUnordered(method, path string, body any) response {
	s.t.Helper()

	resp := s.call(method, path, body)
	s.log[len(s.log)-1].Body = sortedArrays(s.log[len(s.log)-1].Body)

	return resp
}

func (s *session) get(path string) response            { return s.call("GET", path, nil) }
func (s *session) post(path string, body any) response { return s.call("POST", path, body) }
func (s *session) put(path string, body any) response  { return s.call("PUT", path, body) }
func (s *session) delete(path string) response         { return s.call("DELETE", path, nil) }
func (s *session) query(body map[string]any) response  { return s.call("POST", "/api/query", body) }
func (s *session) fail(format string, args ...any) {
	s.t.Helper()
	s.t.Fatalf("[%s] "+format, append([]any{s.engine}, args...)...)
}

func (s *session) expectStatus(resp response, want int) {
	s.t.Helper()

	if resp.Status != want {
		s.fail("expected status %d, got %d: %s", want, resp.Status, resp.Raw)
	}
}

func (s *session) expectIds(resp response, want ...string) {
	s.t.Helper()

	s.expectStatus(resp, 200)
	got := idsOf(resp.JSON)
	if strings.Join(got, ",") != strings.Join(want, ",") {
		s.fail("expected ids %v, got %v (%s)", want, got, resp.Raw)
	}
}

func (s *session) expectCount(resp response, want int) {
	s.t.Helper()

	s.expectStatus(resp, 200)
	m, ok := resp.JSON.(map[string]any)
	if !ok || m["count"] != float64(want) {
		s.fail("expected count %d, got %s", want, resp.Raw)
	}
}

func (s *session) expectJSON(resp response, want string) {
	s.t.Helper()

	var expected any
	if err := json.Unmarshal([]byte(want), &expected); err != nil {
		s.t.Fatalf("bad expectation %q: %v", want, err)
	}

	if canonical(resp.JSON) != canonical(expected) {
		s.fail("expected %s, got %s", canonical(expected), resp.Raw)
	}
}

func idsOf(v any) []string {
	list, _ := v.([]any)
	ids := make([]string, 0, len(list))
	for _, item := range list {
		if m, ok := item.(map[string]any); ok {
			ids = append(ids, fmt.Sprint(m["id"]))
		}
	}

	return ids
}

func field(v any, path string) any {
	cur := v
	for _, p := range strings.Split(path, ".") {
		m, ok := cur.(map[string]any)
		if !ok {
			return nil
		}
		cur = m[p]
	}

	return cur
}

func canonical(v any) string {
	b, _ := json.Marshal(normalizeVolatile(v))

	return string(b)
}

func normalizeVolatile(v any) any {
	switch t := v.(type) {
	case map[string]any:
		out := make(map[string]any, len(t))
		for k, val := range t {
			out[k] = normalizeVolatile(val)
		}
		return out
	case []any:
		out := make([]any, len(t))
		for i, val := range t {
			out[i] = normalizeVolatile(val)
		}
		return out
	case string:
		return uuidPattern.ReplaceAllString(txIdPattern.ReplaceAllString(t, "<tx>"), "<uuid>")
	}

	return v
}

func sortedArrays(v any) any {
	switch t := v.(type) {
	case map[string]any:
		out := make(map[string]any, len(t))
		for k, val := range t {
			out[k] = sortedArrays(val)
		}
		return out
	case []any:
		out := make([]any, len(t))
		for i, val := range t {
			out[i] = sortedArrays(val)
		}
		sort.Slice(out, func(i, j int) bool { return canonical(out[i]) < canonical(out[j]) })
		return out
	}

	return v
}

func compareLogs(t *testing.T, internal, mongo []exchange) {
	t.Helper()

	if len(internal) != len(mongo) {
		t.Fatalf("engines made a different number of calls: internal=%d mongodb=%d", len(internal), len(mongo))
	}

	for i := range internal {
		a, b := internal[i], mongo[i]
		if a.Method != b.Method || normalizeVolatile(a.Path) != normalizeVolatile(b.Path) {
			t.Fatalf("call #%d differs: %s %s vs %s %s", i, a.Method, a.Path, b.Method, b.Path)
		}

		if a.Status != b.Status || canonical(a.Body) != canonical(b.Body) {
			t.Fatalf("engines disagree on call #%d %s %s\ninternal: %d %s\nmongodb:  %d %s",
				i, a.Method, a.Path, a.Status, canonical(a.Body), b.Status, canonical(b.Body))
		}
	}
}
