package compat_test

import (
	"encoding/json"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	tcprouting "github.com/taymour/elysiandb/internal/transport/tcp/tcp_routing"
	"github.com/valyala/fasthttp"
	"github.com/valyala/fasthttp/fasthttputil"
	"net"
)

const (
	legacyFixturesDir = "testdata/v0.2.4"
	legacyGoldenPath  = "testdata/golden_legacy.json"
)

var legacyVariants = []string{"full", "pending", "kv-only", "empty", "inconsistent", "failed-log"}

func copyFolder(t *testing.T, src string) string {
	t.Helper()

	dst := t.TempDir()
	err := filepath.WalkDir(src, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}

		rel, _ := filepath.Rel(src, path)
		target := filepath.Join(dst, rel)
		if d.IsDir() {
			return os.MkdirAll(target, 0o755)
		}

		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}

		return os.WriteFile(target, data, 0o600)
	})
	if err != nil {
		t.Fatalf("cannot copy %s: %v", src, err)
	}

	return dst
}

func startServerOnFolder(t *testing.T, folder string, crashRecovery bool) *corpusServer {
	t.Helper()

	cfg := corpusConfig(folder)
	cfg.Store.CrashRecovery.Enabled = crashRecovery
	cfg.Store.CrashRecovery.MaxLogMB = 100
	globals.SetConfig(cfg)
	security.ResetLoginAttempts()
	boot.InitDB()
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

	local, remote := net.Pipe()
	t.Cleanup(func() {
		_ = ln.Close()
		_ = srv.Shutdown()
		_ = local.Close()
		_ = remote.Close()
		shutdownEngine()
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
		conn:    loopbackConn{Conn: local},
	}

	s.login("admin", security.DefaultAdminUsername, security.DefaultAdminPassword)
	tcprouting.RouteLine([]byte("AUTH "+security.DefaultAdminUsername+" "+security.DefaultAdminPassword+"\n"), s.conn, s.tcp)

	return s
}

func legacyReadRequests() []fixtureRequest {
	reqs := []fixtureRequest{
		{Method: "GET", Path: "/kv/session:1"},
		{Method: "GET", Path: "/kv/derived:total"},
		{Method: "GET", Path: "/kv/long"},
		{Method: "GET", Path: "/kv/pending:key"},
		{Method: "GET", Path: "/kv/mget?keys=session:1,long,derived:total,pending:key"},
		{Method: "GET", Path: "/api/hook/budget_month"},
		{Method: "GET", Path: "/api/acl/reader/note"},
		{Method: "GET", Path: "/api/note"},
		{Method: "GET", Path: "/api/pending"},
		{Method: "GET", Path: "/api/pending/count"},
	}

	return append(reqs, readRequests()...)
}

func TestLegacyFolderCorpus(t *testing.T) {
	record := os.Getenv("ELYSIAN_RECORD_GOLDEN") == "1"

	results := map[string][]goldenEntry{}
	for _, variant := range legacyVariants {
		folder := copyFolder(t, filepath.Join(legacyFixturesDir, variant))
		s := startServerOnFolder(t, folder, variant == "pending")
		entries := []goldenEntry{}
		for _, r := range legacyReadRequests() {
			entries = append(entries, s.run(r))
		}
		results[variant] = entries
	}

	if record {
		data, _ := json.MarshalIndent(results, "", "  ")
		if err := os.WriteFile(legacyGoldenPath, append(data, '\n'), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}

	raw, err := os.ReadFile(legacyGoldenPath)
	if err != nil {
		t.Fatalf("cannot read %s: %v", legacyGoldenPath, err)
	}

	var golden map[string][]goldenEntry
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatal(err)
	}

	for _, variant := range legacyVariants {
		want, got := golden[variant], results[variant]
		if len(want) != len(got) {
			t.Fatalf("%s: %d requests, golden has %d", variant, len(got), len(want))
		}

		failures := 0
		for i := range want {
			if intentionalChange(want[i].Request) {
				continue
			}

			if got[i].Status != want[i].Status || got[i].ContentType != want[i].ContentType || got[i].Body != want[i].Body {
				failures++
				if failures <= 10 {
					t.Errorf("%s #%d %s %s\nwant %d %s\ngot  %d %s", variant, i, want[i].Request.Method, want[i].Request.Path, want[i].Status, truncate(want[i].Body), got[i].Status, truncate(got[i].Body))
				}
			}
		}
	}
}

func TestRecordLegacyFixtures(t *testing.T) {
	if os.Getenv("ELYSIAN_RECORD_FIXTURES") != "1" {
		t.Skip("set ELYSIAN_RECORD_FIXTURES=1 to write the 0.2.4 data folders")
	}

	for _, variant := range legacyVariants {
		target := filepath.Join(legacyFixturesDir, variant)
		_ = os.RemoveAll(target)
		if err := os.MkdirAll(target, 0o755); err != nil {
			t.Fatal(err)
		}

		switch variant {
		case "inconsistent":
			deriveInconsistentVariant(t, filepath.Join(legacyFixturesDir, "full"), target)
		case "failed-log":
			deriveFailedLogVariant(t, filepath.Join(legacyFixturesDir, "full"), target)
		default:
			writeLegacyVariant(t, variant, target)
		}
	}
}

func copyFiles(t *testing.T, src, dst string) {
	t.Helper()

	entries, err := os.ReadDir(src)
	if err != nil {
		t.Fatal(err)
	}

	for _, e := range entries {
		data, err := os.ReadFile(filepath.Join(src, e.Name()))
		if err != nil {
			t.Fatal(err)
		}

		if err := os.WriteFile(filepath.Join(dst, e.Name()), data, 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

func deriveInconsistentVariant(t *testing.T, src, dst string) {
	t.Helper()

	copyFiles(t, src, dst)

	path := filepath.Join(dst, "elysiandb.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	var kv map[string][]byte
	if err := json.Unmarshal(raw, &kv); err != nil {
		t.Fatal(err)
	}

	key := "api:entity:book:internal:index:id"
	ids := strings.Split(string(kv[key]), "\n")
	kept := []string{}
	for _, id := range ids {
		if id != "b03" && id != "b11" {
			kept = append(kept, id)
		}
	}
	kept = append(kept, "ghost")
	kv[key] = []byte(strings.Join(kept, "\n"))

	types := strings.Split(string(kv["api:entity:types:list"]), "\n")
	keptTypes := []string{}
	for _, entity := range types {
		if entity != "review" {
			keptTypes = append(keptTypes, entity)
		}
	}
	kv["api:entity:types:list"] = []byte(strings.Join(keptTypes, "\n"))

	out, _ := json.Marshal(kv)
	if err := os.WriteFile(path, append(out, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}

func deriveFailedLogVariant(t *testing.T, src, dst string) {
	t.Helper()

	copyFiles(t, src, dst)

	if err := os.WriteFile(filepath.Join(dst, "elysiandb.json.recovery.log.failed"), []byte("{not json\n"), 0o644); err != nil {
		t.Fatal(err)
	}
}

func writeLegacyVariant(t *testing.T, variant, target string) {
	t.Helper()

	folder := t.TempDir()
	s := startServerOnFolder(t, folder, variant == "pending")

	if variant == "full" || variant == "pending" {
		for _, r := range append(append(fixtureLoadRequests(), hookRequests()...), fixtureRequest{Method: "POST", Path: "/api/security/user", Body: `{"username":"reader","password":"reader-password-1","role":"user"}`}) {
			s.run(r)
		}
		s.run(fixtureRequest{Method: "POST", Path: "/api/note", Body: `{"id":"n1","text":"admin note"}`})
		s.run(fixtureRequest{Method: "PUT", Path: "/api/acl/reader/note", Body: `{"permissions":{"create":true,"read":false,"update":false,"delete":false,"owning_read":true,"owning_write":true,"owning_update":true,"owning_delete":true}}`})
	}

	if variant != "empty" {
		for _, r := range []fixtureRequest{
			{Method: "PUT", Path: "/kv/session:1", Body: "alpha"},
			{Method: "PUT", Path: "/kv/derived:total", Body: "42"},
			{Method: "PUT", Path: "/kv/long?ttl=900000000", Body: "kept for years"},
			{Method: "PUT", Path: "/kv/soon?ttl=1", Body: "expires"},
		} {
			s.run(r)
		}
	}

	s.run(fixtureRequest{Method: "POST", Path: "/save"})

	if variant == "pending" {
		for _, r := range []fixtureRequest{
			{Method: "PUT", Path: "/kv/pending:key", Body: "written after the flush"},
			{Method: "POST", Path: "/api/pending", Body: `[{"id":"x1","v":1},{"id":"x2","v":2}]`},
			{Method: "PUT", Path: "/api/book/b07", Body: `{"status":"pending-update"}`},
			{Method: "DELETE", Path: "/api/book/b08"},
		} {
			s.run(r)
		}
	}

	time.Sleep(1100 * time.Millisecond)

	entries, err := os.ReadDir(folder)
	if err != nil {
		t.Fatal(err)
	}

	for _, e := range entries {
		if e.IsDir() || strings.HasSuffix(e.Name(), ".tmp") {
			continue
		}

		data, err := os.ReadFile(filepath.Join(folder, e.Name()))
		if err != nil {
			t.Fatal(err)
		}

		if err := os.WriteFile(filepath.Join(target, e.Name()), data, 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

func shutdownEngine() {
	_ = storage.Close()
}
