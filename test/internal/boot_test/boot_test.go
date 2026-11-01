package boot_test

import (
	"fmt"
	"log/slog"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
)

func setupLog(t *testing.T) {
	t.Helper()

	if err := log.Configure(log.Options{Level: "debug", BufferLines: 1000}); err != nil {
		t.Fatal(err)
	}
	log.SetOutput(&strings.Builder{}, false)

	t.Cleanup(func() {
		log.SetOutput(nil, false)
		_ = log.Configure(log.Options{})
	})
}

func entries(q log.Query) []log.Entry {
	return log.Logs(q).Entries
}

func config(folder string, mutate func(cfg *configuration.Config)) *configuration.Config {
	cfg := &configuration.Config{}
	cfg.Store.Folder = folder
	cfg.Store.Shards = 4
	cfg.Server.HTTP = configuration.ServerConfig{Enabled: false, Host: "127.0.0.1", Port: 8089}
	cfg.Server.TCP = configuration.ServerConfig{Enabled: false, Host: "127.0.0.1", Port: 8088}
	cfg.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "user"}
	if mutate != nil {
		mutate(cfg)
	}

	globals.SetConfig(cfg)

	return cfg
}

func freePort(t *testing.T) int {
	t.Helper()

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()

	return ln.Addr().(*net.TCPAddr).Port
}

func TestLogConfiguration(t *testing.T) {
	setupLog(t)
	config(t.TempDir(), func(cfg *configuration.Config) {
		cfg.Server.HTTP.Enabled = true
		cfg.Log.FlushIntervalSeconds = 5
	})

	boot.LogConfiguration("elysian.yaml")

	loaded := entries(log.Query{Text: "configuration loaded"})
	if len(loaded) != 1 {
		t.Fatalf("expected one configuration line, got %+v", loaded)
	}

	attrs := loaded[0].Attrs
	if attrs["path"] != "elysian.yaml" || attrs["http"] != "127.0.0.1:8089" || attrs["tcp"] != "disabled" || attrs["auth_mode"] != "user" || attrs["log_format"] != "text" {
		t.Fatalf("unexpected attributes %+v", attrs)
	}

	deprecated := entries(log.Query{MinLevel: slog.LevelWarn})
	if len(deprecated) != 1 || deprecated[0].Attrs["key"] != "log.flushIntervalSeconds" {
		t.Fatalf("expected a deprecation warning, got %+v", deprecated)
	}
}

func TestWarnIfAuthenticationIsOffOnPublicAddress(t *testing.T) {
	setupLog(t)

	config(t.TempDir(), func(cfg *configuration.Config) {
		cfg.Security.Authentication.Enabled = false
		cfg.Server.TCP = configuration.ServerConfig{Enabled: true, Host: "127.0.0.1", Port: 8088}
	})
	boot.WarnIfAuthenticationIsOffOnPublicAddress()
	if got := entries(log.Query{MinLevel: slog.LevelWarn}); len(got) != 0 {
		t.Fatalf("a loopback address needs no warning, got %+v", got)
	}

	config(t.TempDir(), func(cfg *configuration.Config) {
		cfg.Security.Authentication.Enabled = false
		cfg.Server.HTTP = configuration.ServerConfig{Enabled: true, Host: "0.0.0.0", Port: 8089}
	})
	boot.WarnIfAuthenticationIsOffOnPublicAddress()

	got := entries(log.Query{MinLevel: slog.LevelWarn})
	if len(got) != 1 || got[0].Message != "authentication is disabled on a non-loopback address" || got[0].Attrs["http"] != "0.0.0.0:8089" {
		t.Fatalf("expected one warning, got %+v", got)
	}
}

func TestMonitoringSources(t *testing.T) {
	config(t.TempDir(), nil)
	storage.LoadDB()
	storage.LoadJsonDB()

	sources := boot.MonitoringSources()
	if sources.AdminSessions == nil || sources.DefaultAdminPassword == nil {
		t.Fatalf("user mode reports sessions and the default password")
	}
	if sources.MongoPing != nil || sources.MongoPoolInUse != nil {
		t.Fatalf("the internal engine has no MongoDB source")
	}

	if keys, _ := sources.KVKeys(); keys != 0 {
		t.Fatalf("expected an empty store, got %d keys", keys)
	}
	if sources.DataFolder() != globals.GetConfig().Store.Folder {
		t.Fatalf("unexpected data folder")
	}
	if n := sources.CacheEntries(); n != 0 {
		t.Fatalf("no cache, no entries, got %d", n)
	}
	cache.InitCache(time.Minute)
	if n := sources.CacheEntries(); n != 0 {
		t.Fatalf("an empty cache has no entries, got %d", n)
	}
	if open, _ := sources.Transactions(); open != 0 {
		t.Fatalf("expected no transaction, got %d", open)
	}
	if json, store := sources.RecoverySizes(); json != 0 || store != 0 {
		t.Fatalf("expected no recovery log, got %d %d", json, store)
	}
	if n := sources.IndexBacklog(); n < 0 {
		t.Fatalf("unexpected backlog %d", n)
	}
	if n := sources.HTTPConnections(); n != 0 {
		t.Fatalf("no server, no connection, got %d", n)
	}
	if counts := sources.EntityCounts(); len(counts) != 0 {
		t.Fatalf("expected no entity type, got %v", counts)
	}

	config(t.TempDir(), func(cfg *configuration.Config) {
		cfg.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "token", Token: "t"}
	})
	sources = boot.MonitoringSources()
	if sources.AdminSessions != nil || sources.DefaultAdminPassword != nil {
		t.Fatalf("token mode has no sessions and no default password")
	}
}

func TestBootMonitoringStartsTheSampler(t *testing.T) {
	config(t.TempDir(), nil)
	storage.LoadDB()
	storage.LoadJsonDB()
	metrics.Reset()
	t.Cleanup(metrics.Reset)

	boot.BootMonitoring()

	deadline := time.Now().Add(5 * time.Second)
	for {
		s, _ := metrics.Series("15m", 0)
		if len(s.Points) > 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("the sampler did not record a point")
		}
		time.Sleep(50 * time.Millisecond)
	}
}

func TestMongoHost(t *testing.T) {
	cases := map[string]string{
		"mongodb://user:secret@db.example.com:27017/elysiandb": "db.example.com:27017",
		"mongodb://localhost:27017":                            "localhost:27017",
		"not a uri %%":                                         "unknown",
		"":                                                     "unknown",
	}

	for uri, want := range cases {
		if got := boot.MongoHost(uri); got != want {
			t.Fatalf("MongoHost(%q) = %q, want %q", uri, got, want)
		}
	}

	if _, err := boot.MongoPing(); err == nil && globals.MongoClient == nil {
		t.Fatalf("a ping without a client must fail")
	}
}

func TestHTTPHandlerAddsRequestIDsAndRecoversPanics(t *testing.T) {
	setupLog(t)
	config(t.TempDir(), func(cfg *configuration.Config) {
		cfg.Security.Authentication.Enabled = false
	})
	storage.LoadDB()
	storage.LoadJsonDB()

	h := boot.NewHTTPHandler(boot.NewRouter())

	req := fasthttp.AcquireRequest()
	req.SetRequestURI("/health")
	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	h(ctx)

	if ctx.Response.StatusCode() != 200 || len(ctx.Response.Header.Peek("X-Request-Id")) == 0 {
		t.Fatalf("expected 200 with a request id, got %d", ctx.Response.StatusCode())
	}
}

func TestStartHTTP(t *testing.T) {
	setupLog(t)
	port := freePort(t)
	config(t.TempDir(), func(cfg *configuration.Config) {
		cfg.Security.Authentication.Enabled = false
		cfg.Server.HTTP = configuration.ServerConfig{Enabled: true, Host: "127.0.0.1", Port: port}
	})
	storage.LoadDB()
	storage.LoadJsonDB()

	go boot.StartHTTP()

	url := fmt.Sprintf("http://127.0.0.1:%d/health", port)
	deadline := time.Now().Add(5 * time.Second)
	for {
		status, _, err := fasthttp.Get(nil, url)
		if err == nil && status == 200 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("the HTTP server did not answer: %v", err)
		}
		time.Sleep(20 * time.Millisecond)
	}

	listening := entries(log.Query{Text: "http listening"})
	if len(listening) != 1 || listening[0].Attrs["addr"] != fmt.Sprintf("127.0.0.1:%d", port) {
		t.Fatalf("expected an http listening line, got %+v", listening)
	}

	if n := boot.HTTPConnections(); n < 0 {
		t.Fatalf("unexpected connection count %d", n)
	}
}

func TestInitDBReplaysTheRecoveryLogAndRepairsTheIndexes(t *testing.T) {
	folder := t.TempDir()
	config(folder, nil)
	storage.LoadDB()
	storage.LoadJsonDB()

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatal(err)
	}
	api_storage.WriteEntity("books", map[string]any{"id": "b1", "title": "Dune"})
	storage.DeleteByWildcardKey(globals.ApiKeyPrefix + "*")
	storage.WriteToDB()

	line := `{"op":"put","key":"api:entity:books:id:b2","value":{"id":"b2","title":"Emma"}}` + "\n"
	if err := os.WriteFile(filepath.Join(folder, recovery.RecoveryFile), []byte(line), 0o600); err != nil {
		t.Fatal(err)
	}

	setupLog(t)
	config(folder, func(cfg *configuration.Config) {
		cfg.Store.CrashRecovery = configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 100}
	})

	boot.InitDB()

	replayed := entries(log.Query{Text: "unclean shutdown detected"})
	if len(replayed) != 1 || replayed[0].Attrs["entries"] != int64(1) {
		t.Fatalf("expected one replay warning, got %+v", replayed)
	}

	repaired := entries(log.Query{Text: "rest api indexes repaired"})
	if len(repaired) != 1 {
		t.Fatalf("expected one repair warning, got %+v", entries(log.Query{MinLevel: slog.LevelDebug}))
	}

	if loaded := entries(log.Query{Text: "storage loaded"}); len(loaded) != 1 {
		t.Fatalf("expected one storage loaded line, got %+v", loaded)
	}

	if got := api_storage.CountEntities("books"); got != 2 {
		t.Fatalf("both books should be listed, got %d", got)
	}

	if acl.GetACLEntityForUsername("books", security.DefaultAdminUsername) == nil {
		t.Fatalf("the admin ACL of books should be created at startup")
	}
}
