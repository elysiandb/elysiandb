package mongodb_boot_test

import (
	"context"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/test/mongotest"
	"github.com/valyala/fasthttp"
	"github.com/valyala/fasthttp/fasthttputil"
	"go.mongodb.org/mongo-driver/v2/bson"
)

func request(t *testing.T, c *fasthttp.Client, method, path string, body any, cookie string) (int, []byte) {
	t.Helper()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(method)
	req.SetRequestURI("http://elysian" + path)
	if cookie != "" {
		req.Header.SetCookie(security.SessionCookieName, cookie)
	}
	if body != nil {
		b, _ := json.Marshal(body)
		req.SetBody(b)
	}

	if err := c.DoTimeout(req, resp, 30*time.Second); err != nil {
		t.Fatalf("%s %s: %v", method, path, err)
	}

	return resp.StatusCode(), append([]byte(nil), resp.Body()...)
}

func login(t *testing.T, c *fasthttp.Client) string {
	t.Helper()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod("POST")
	req.SetRequestURI("http://elysian/api/security/login")
	req.SetBodyString(`{"username":"admin","password":"admin"}`)
	if err := c.DoTimeout(req, resp, 30*time.Second); err != nil {
		t.Fatalf("login: %v", err)
	}

	if resp.StatusCode() != 200 {
		t.Fatalf("admin login failed with %d", resp.StatusCode())
	}

	cookie := fasthttp.AcquireCookie()
	defer fasthttp.ReleaseCookie(cookie)
	cookie.SetKey(security.SessionCookieName)
	if !resp.Header.Cookie(cookie) {
		t.Fatalf("login did not set a session cookie")
	}

	return string(cookie.Value())
}

func TestMongoDBEngineBootsWithoutTheInternalDocumentStore(t *testing.T) {
	uri := mongotest.URI(t)
	boot.MongoDatabaseName = mongotest.DatabaseName(t)
	folder := t.TempDir()

	if err := os.WriteFile(filepath.Join(folder, security.KeyFilename), []byte("legacy-local-key"), 0o644); err != nil {
		t.Fatalf("write legacy key: %v", err)
	}

	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder:               folder,
			Shards:               8,
			FlushIntervalSeconds: 1,
			CrashRecovery:        configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 10},
		},
		Engine:   configuration.EngineConfig{Name: "mongodb", URI: uri},
		Server:   configuration.ServersConfig{HTTP: configuration.ServerConfig{Enabled: true}},
		Security: configuration.SecurityConfig{Authentication: configuration.AuthenticationConfig{Enabled: true, Mode: "user"}},
		Api: configuration.ApiConfig{
			Schema: configuration.ApiSchemaConfig{Enabled: true},
			Hooks:  configuration.HooksConfig{Enabled: true},
			Index:  configuration.ApiIndexConfig{Workers: 2},
		},
	})

	boot.InitDB()
	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatalf("init admin: %v", err)
	}
	t.Cleanup(func() {
		_ = globals.MongoDB.Drop(context.Background())
		_ = globals.MongoClient.Disconnect(context.Background())
	})

	if _, err := storage.GetJsonByKeyNoCopy("anything"); err == nil || err.Error() != "json store not initialized" {
		t.Fatalf("expected the internal JSON store to stay unloaded, got %v", err)
	}

	if globals.MongoDB.Name() != boot.MongoDatabaseName {
		t.Fatalf("connected to %q instead of %q", globals.MongoDB.Name(), boot.MongoDatabaseName)
	}

	r := router.New()
	routing.RegisterRoutes(r)
	srv := &fasthttp.Server{Handler: r.Handler}
	ln := fasthttputil.NewInmemoryListener()
	go func() { _ = srv.Serve(ln) }()
	defer func() { _ = ln.Close() }()
	c := &fasthttp.Client{Dial: func(string) (net.Conn, error) { return ln.Dial() }}

	key, err := mongodb.GetOrCreateSetting(security.UsersKeySetting, func() (string, error) { return "unexpected", nil })
	if err != nil || key != "legacy-local-key" {
		t.Fatalf("expected the existing local users key to be moved to MongoDB, got %q (%v)", key, err)
	}

	cookie := login(t, c)

	if status, body := request(t, c, "PUT", "/kv/greeting", "hello", cookie); status != 204 && status != 200 {
		t.Fatalf("kv put: %d %s", status, body)
	}
	if status, body := request(t, c, "GET", "/kv/greeting", nil, cookie); status != 200 || string(body) != `{"key":"greeting","value":"\"hello\""}` {
		t.Fatalf("kv get: %d %s", status, body)
	}

	if status, body := request(t, c, "POST", "/api/user", []any{
		map[string]any{"id": "u1", "name": "Alice", "age": 30, "company": map[string]any{"@entity": "company", "id": "c1", "name": "Acme"}},
		map[string]any{"id": "u2", "name": "Bob", "age": 25},
	}, cookie); status != 200 {
		t.Fatalf("create users: %d %s", status, body)
	}
	if status, body := request(t, c, "GET", "/api/user?sort[age]=asc", nil, cookie); status != 200 {
		t.Fatalf("sorted list: %d %s", status, body)
	}

	time.Sleep(1500 * time.Millisecond)

	if keys := storage.GetByWildcardKey("api:*"); len(keys) > 0 {
		t.Fatalf("API metadata written to the internal KV store: %v", keys)
	}

	for _, name := range []string{storage.JsonDataFile, recovery.RecoveryFile, security.KeyFilename + ".new"} {
		if _, err := os.Stat(filepath.Join(folder, name)); !os.IsNotExist(err) {
			t.Fatalf("expected no %s in MongoDB mode, got %v", name, err)
		}
	}

	persisted, err := os.ReadFile(filepath.Join(folder, storage.DataFile))
	if err != nil || !strings.Contains(string(persisted), "greeting") {
		t.Fatalf("expected the KV store to be flushed to disk in MongoDB mode, got %q (%v)", persisted, err)
	}

	collections, err := globals.MongoDB.ListCollectionNames(context.Background(), bson.M{})
	if err != nil {
		t.Fatalf("list collections: %v", err)
	}
	slices.Sort(collections)
	for _, want := range []string{"_elysiandb_core_hook", "_elysiandb_core_schema", "company", "user"} {
		if !slices.Contains(collections, want) {
			t.Fatalf("expected collection %q in %v", want, collections)
		}
	}

	storage.ResetStore()
	if err := os.RemoveAll(folder); err != nil {
		t.Fatalf("remove store folder: %v", err)
	}
	if err := os.MkdirAll(folder, 0o755); err != nil {
		t.Fatalf("recreate store folder: %v", err)
	}

	cookie = login(t, c)

	status, body := request(t, c, "GET", "/api/entity/types/name", nil, cookie)
	if status != 200 || string(body) != `{"entities":["company","user"]}` {
		t.Fatalf("entity types after losing local files: %d %s", status, body)
	}

	status, body = request(t, c, "GET", "/api/user?sort[age]=desc&includes=company&fields=id,company.name", nil, cookie)
	if status != 200 || string(body) != `[{"company":{"name":"Acme"},"id":"u1"},{"id":"u2"}]` {
		t.Fatalf("sorted list with includes after losing local files: %d %s", status, body)
	}

	status, body = request(t, c, "GET", "/api/user/schema", nil, cookie)
	if status != 200 {
		t.Fatalf("schema after losing local files: %d %s", status, body)
	}
}
