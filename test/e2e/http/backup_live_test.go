package e2e

import (
	"archive/tar"
	"bufio"
	"bytes"
	"compress/gzip"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"mime/multipart"
	"net"
	"net/http"
	"net/http/cookiejar"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/cmd"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

type liveServer struct {
	t    *testing.T
	url  string
	addr string
	http *http.Client
}

func startLiveServer(t *testing.T, configure func(cfg *configuration.Config)) *liveServer {
	t.Helper()

	cfg := defaultAPIConfig(t.TempDir())
	cfg.Store.CrashRecovery = configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 100}
	cfg.Api.Schema = configuration.ApiSchemaConfig{Enabled: false}
	if configure != nil {
		configure(cfg)
	}
	globals.SetConfig(cfg)
	security.ResetLoginAttempts()
	if err := storage.Open(storage.ModeServer); err != nil {
		t.Fatal(err)
	}
	cache.InitCache(10 * time.Second)
	hook.InitHooks()
	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatal(err)
	}
	_ = acl.InitACL()

	ln, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}

	srv := boot.NewHTTPServer()
	go func() { _ = srv.Serve(ln) }()

	jar, _ := cookiejar.New(nil)
	s := &liveServer{t: t, addr: ln.Addr().String(), url: "http://" + ln.Addr().String(), http: &http.Client{Jar: jar}}

	t.Cleanup(func() {
		_ = ln.Close()
		_ = srv.Shutdown()
		_ = storage.Close()
	})

	return s
}

func (s *liveServer) login() {
	s.t.Helper()

	resp, err := s.http.Post(s.url+"/api/security/login", "application/json", strings.NewReader(`{"username":"admin","password":"admin"}`))
	if err != nil {
		s.t.Fatal(err)
	}
	resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		s.t.Fatalf("login: %s", resp.Status)
	}
}

func (s *liveServer) do(method, path string, body io.Reader, contentType string) (int, []byte) {
	s.t.Helper()

	req, err := http.NewRequest(method, s.url+path, body)
	if err != nil {
		s.t.Fatal(err)
	}
	if contentType != "" {
		req.Header.Set("Content-Type", contentType)
	}

	resp, err := s.http.Do(req)
	if err != nil {
		s.t.Fatal(err)
	}
	defer resp.Body.Close()

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		s.t.Fatal(err)
	}

	return resp.StatusCode, data
}

func archiveFiles(t *testing.T, archive []byte) (map[string][]byte, []string) {
	t.Helper()

	gz, err := gzip.NewReader(bytes.NewReader(archive))
	if err != nil {
		t.Fatal(err)
	}

	files := map[string][]byte{}
	order := []string{}
	tr := tar.NewReader(gz)
	for {
		h, err := tr.Next()
		if err == io.EOF {
			return files, order
		}
		if err != nil {
			t.Fatal(err)
		}

		data, err := io.ReadAll(tr)
		if err != nil {
			t.Fatal(err)
		}
		files[h.Name] = data
		order = append(order, h.Name)
	}
}

func documentIDs(t *testing.T, data []byte) map[string]bool {
	t.Helper()

	ids := map[string]bool{}
	dec := json.NewDecoder(bytes.NewReader(data))
	for {
		var doc map[string]any
		if err := dec.Decode(&doc); err == io.EOF {
			return ids
		} else if err != nil {
			t.Fatal(err)
		}
		ids[doc["id"].(string)] = true
	}
}

func TestLiveBackupHoldsOneInstantUnderWrites(t *testing.T) {
	s := startLiveServer(t, nil)
	s.login()

	e := storage.Engine()
	pad := strings.Repeat("x", 400)
	put := func(n int) error {
		id := fmt.Sprintf("n%07d", n)
		return e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("left")
			tx.EnsureType("right")
			if err := tx.Put("left", id, map[string]any{"id": id, "n": float64(n), "pad": pad}); err != nil {
				return err
			}
			return tx.Put("right", id, map[string]any{"id": id, "n": float64(n), "pad": pad})
		})
	}

	for n := 1; n <= 30000; n += 1000 {
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("left")
			tx.EnsureType("right")
			for i := n; i < n+1000; i++ {
				id := fmt.Sprintf("n%07d", i)
				if err := tx.Put("left", id, map[string]any{"id": id, "n": float64(i), "pad": pad}); err != nil {
					return err
				}
				if err := tx.Put("right", id, map[string]any{"id": id, "n": float64(i), "pad": pad}); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}

	var acked atomic.Int64
	acked.Store(30000)
	var stop atomic.Bool
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for n := 30001; !stop.Load(); n++ {
			if err := put(n); err != nil {
				t.Errorf("write %d: %v", n, err)
				return
			}
			if err := e.KVPut("counter", []byte(strconv.Itoa(n)), 0); err != nil {
				t.Errorf("key %d: %v", n, err)
				return
			}
			acked.Store(int64(n))
		}
	}()

	time.Sleep(20 * time.Millisecond)
	before := acked.Load()
	status, archive := s.do("GET", "/api/backup", nil, "")
	after := acked.Load()
	stop.Store(true)
	wg.Wait()

	if status != http.StatusOK {
		t.Fatalf("backup: %d", status)
	}

	files, _ := archiveFiles(t, archive)
	left := documentIDs(t, files[backup.EntityPath("left")])
	right := documentIDs(t, files[backup.EntityPath("right")])
	if len(left) != len(right) {
		t.Fatalf("a transaction is torn in the archive: %d left, %d right", len(left), len(right))
	}

	for id := range left {
		if !right[id] {
			t.Fatalf("%s is only on one side", id)
		}
	}

	for n := 1; n <= len(left); n++ {
		if !left[fmt.Sprintf("n%07d", n)] {
			t.Fatalf("hole at %d: the archive is not one instant of a sequence of %d writes", n, len(left))
		}
	}

	if int64(len(left)) < before {
		t.Fatalf("a write confirmed before the backup is missing: %d documents, %d confirmed", len(left), before)
	}

	if int64(len(left)) > after+1 {
		t.Fatalf("the archive holds writes made after it: %d documents, %d confirmed at the end", len(left), after)
	}

	var counter string
	for _, line := range bytes.Split(files[backup.KVPath], []byte("\n")) {
		var kv backup.KVEntry
		if json.Unmarshal(line, &kv) == nil && kv.Key == "counter" {
			counter = kv.Value
		}
	}

	if got, _ := strconv.Atoi(counter); int64(got) < before || got > len(left) || got < len(left)-1 {
		t.Fatalf("the key is not of the same instant: %q for %d documents", counter, len(left))
	}

	if int64(len(left)) == after && after-before == 0 {
		t.Log("no write landed during this backup")
	}

	snapshot := map[string][]byte{}
	liveSnap := e.Snapshot("left")
	for id := range left {
		d, ok := liveSnap.Get(id)
		if !ok {
			t.Fatalf("%s is gone", id)
		}
		raw, err := d.Bytes()
		if err != nil {
			t.Fatal(err)
		}
		snapshot[id] = append([]byte(nil), raw...)
	}

	status, report := s.do("POST", "/api/restore", bytes.NewReader(archive), "application/gzip")
	if status != http.StatusOK {
		t.Fatalf("restore: %d %s", status, report)
	}

	restored := storage.Engine().Snapshot("left")
	if restored.Len() != len(left) || storage.Engine().Snapshot("right").Len() != len(right) {
		t.Fatalf("restored %d, archive %d", restored.Len(), len(left))
	}

	for id, want := range snapshot {
		d, ok := restored.Get(id)
		if !ok {
			t.Fatalf("%s is missing after the restore", id)
		}
		got, err := d.Bytes()
		if err != nil || !bytes.Equal(got, want) {
			t.Fatalf("%s differs after the restore: %s", id, got)
		}
	}

	if value, _, ok := storage.Engine().KVGet("counter"); !ok || string(value) != counter {
		t.Fatalf("key %q after the restore, %q in the archive", value, counter)
	}

	again, _ := archiveFiles(t, mustBackup(t))
	if !bytes.Equal(again[backup.EntityPath("left")], files[backup.EntityPath("left")]) {
		t.Fatalf("a backup of the restored data holds other bytes")
	}
}

func mustBackup(t *testing.T) []byte {
	t.Helper()

	var out bytes.Buffer
	if _, err := backup.Create(&out, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}

	return out.Bytes()
}

func randomPayload(n int) string {
	b := make([]byte, n/2)
	_, _ = rand.Read(b)

	return hex.EncodeToString(b)
}

func fillRandom(t *testing.T, docs, size int) {
	t.Helper()

	e := storage.Engine()
	for n := 0; n < docs; n += 100 {
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("big")
			for i := n; i < n+100 && i < docs; i++ {
				id := fmt.Sprintf("b%06d", i)
				if err := tx.Put("big", id, map[string]any{"id": id, "data": randomPayload(size)}); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
}

func TestLargeArchivesTravelWithoutTheLimitsOfTheOtherRoutes(t *testing.T) {
	s := startLiveServer(t, nil)
	s.login()
	fillRandom(t, 2000, 8000)

	status, archive := s.do("GET", "/api/backup", nil, "")
	if status != http.StatusOK || len(archive) < 6<<20 {
		t.Fatalf("backup: %d, %d bytes", status, len(archive))
	}

	check := func(label string) {
		t.Helper()
		if n := storage.Engine().Snapshot("big").Len(); n != 2000 {
			t.Fatalf("%s: %d documents", label, n)
		}
	}

	if status, body := s.do("POST", "/api/restore", bytes.NewReader(archive), "application/gzip"); status != http.StatusOK {
		t.Fatalf("raw restore: %d %s", status, body)
	}
	s.login()
	check("raw")

	if status, body := s.do("POST", "/api/restore", struct{ io.Reader }{bytes.NewReader(archive)}, "application/gzip"); status != http.StatusOK {
		t.Fatalf("chunked restore: %d %s", status, body)
	}
	s.login()
	check("chunked")

	var form bytes.Buffer
	mw := multipart.NewWriter(&form)
	part, _ := mw.CreateFormFile("archive", "backup.tar.gz")
	_, _ = part.Write(archive)
	_ = mw.Close()
	if status, body := s.do("POST", "/api/restore", &form, mw.FormDataContentType()); status != http.StatusOK {
		t.Fatalf("multipart restore: %d %s", status, body)
	}
	s.login()
	check("multipart")

	limited, err := net.Dial("tcp", s.addr)
	if err != nil {
		t.Fatal(err)
	}
	defer limited.Close()
	fmt.Fprintf(limited, "POST /api/big HTTP/1.1\r\nHost: x\r\nCookie: %s\r\nContent-Length: %d\r\nContent-Type: application/json\r\n\r\n", s.cookieHeader(), 5<<20)
	answer, err := http.ReadResponse(bufio.NewReader(limited), nil)
	if err != nil {
		t.Fatal(err)
	}
	limitBody, _ := io.ReadAll(answer.Body)
	answer.Body.Close()
	if answer.StatusCode != http.StatusBadRequest || !strings.Contains(string(limitBody), "Error when parsing request") {
		t.Fatalf("the other routes keep their limit: %d %s", answer.StatusCode, limitBody)
	}

	if status, body := s.do("POST", "/api/restore", strings.NewReader(strings.Repeat("not an archive", 1<<18)), "application/gzip"); status != http.StatusBadRequest {
		t.Fatalf("garbage: %d %s", status, body)
	}
	check("garbage leaves the data untouched")

	if status, body := s.do("POST", "/api/restore", strings.NewReader(""), "application/gzip"); status != http.StatusBadRequest || !strings.Contains(string(body), "must be a backup archive") {
		t.Fatalf("empty body: %d %s", status, body)
	}

	var noFile bytes.Buffer
	mw = multipart.NewWriter(&noFile)
	_ = mw.WriteField("note", "no file")
	_ = mw.Close()
	if status, body := s.do("POST", "/api/restore", &noFile, mw.FormDataContentType()); status != http.StatusBadRequest || !strings.Contains(string(body), "holds no file") {
		t.Fatalf("multipart without a file: %d %s", status, body)
	}

	conn, err := net.Dial("tcp", s.addr)
	if err != nil {
		t.Fatal(err)
	}
	fmt.Fprintf(conn, "POST /api/restore HTTP/1.1\r\nHost: x\r\nCookie: %s\r\nContent-Length: %d\r\nContent-Type: application/gzip\r\n\r\n", s.cookieHeader(), len(archive))
	_, _ = conn.Write(archive[:len(archive)/2])
	_ = conn.Close()
	time.Sleep(300 * time.Millisecond)
	check("an interrupted upload leaves the data untouched")

	left, _ := filepath.Glob(filepath.Join(globals.GetConfig().Store.Folder, ".restore-upload-*"))
	if len(left) != 0 {
		t.Fatalf("uploads are removed: %v", left)
	}

	anonymous := &http.Client{}
	resp, err := anonymous.Post(s.url+"/api/restore", "application/gzip", bytes.NewReader(archive))
	if err != nil {
		if !strings.Contains(err.Error(), "broken pipe") && !strings.Contains(err.Error(), "connection reset") {
			t.Fatal(err)
		}
	} else {
		resp.Body.Close()
		if resp.StatusCode != http.StatusUnauthorized {
			t.Fatalf("anonymous restore: %s", resp.Status)
		}
	}
	check("a refused restore leaves the data untouched")
}

func (s *liveServer) cookieHeader() string {
	req, _ := http.NewRequest("GET", s.url, nil)
	parts := []string{}
	for _, c := range s.http.Jar.Cookies(req.URL) {
		parts = append(parts, c.Name+"="+c.Value)
	}

	return strings.Join(parts, "; ")
}

func TestABackupCanTakeLongerThanTheWriteTimeout(t *testing.T) {
	s := startLiveServer(t, func(cfg *configuration.Config) {
		cfg.Server.HTTP.WriteTimeoutSeconds = 1
	})
	s.login()
	fillRandom(t, 3000, 8000)

	conn, err := net.Dial("tcp", s.addr)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	_ = conn.(*net.TCPConn).SetReadBuffer(16 << 10)

	fmt.Fprintf(conn, "GET /api/backup HTTP/1.1\r\nHost: x\r\nCookie: %s\r\nConnection: close\r\n\r\n", s.cookieHeader())

	start := time.Now()
	slow := &slowReader{r: conn, chunk: 64 << 10, pause: 25 * time.Millisecond}
	resp, err := http.ReadResponse(bufio.NewReaderSize(slow, 64<<10), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer resp.Body.Close()

	archive, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("after %s: %v", time.Since(start), err)
	}

	if took := time.Since(start); took < 1500*time.Millisecond {
		t.Fatalf("the test should outlast the write timeout, took %s", took)
	}

	if _, err := backup.Verify(bytes.NewReader(archive)); err != nil {
		t.Fatal(err)
	}
}

type slowReader struct {
	r     io.Reader
	chunk int
	pause time.Duration
}

func (s *slowReader) Read(p []byte) (int, error) {
	if len(p) > s.chunk {
		p = p[:s.chunk]
	}
	time.Sleep(s.pause)

	return s.r.Read(p)
}

func TestTheCommandBacksUpARunningServer(t *testing.T) {
	s := startLiveServer(t, nil)
	fillRandom(t, 50, 100)

	dir := t.TempDir()
	t.Setenv(cmd.RemotePasswordEnv, "admin")
	path, err := cmd.RunBackup([]string{"--url", s.url, "--user", "admin", "--output", dir})
	if err != nil {
		t.Fatal(err)
	}

	f, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()

	manifest, err := backup.Verify(f)
	if err != nil {
		t.Fatal(err)
	}

	found := false
	for _, entity := range manifest.Entities {
		found = found || (entity.Name == "big" && entity.Documents == 50)
	}
	if !found {
		t.Fatalf("manifest %+v", manifest.Entities)
	}

	left, _ := filepath.Glob(filepath.Join(dir, ".*"))
	if len(left) != 0 {
		t.Fatalf("temporary files are removed: %v", left)
	}

	if _, err := cmd.RunBackup([]string{"--url", s.url, "--user", "admin", "--output", filepath.Join(dir, "missing")}); err == nil {
		t.Fatalf("a missing folder is refused")
	}

	t.Setenv(cmd.RemotePasswordEnv, "wrong")
	if _, err := cmd.RunBackup([]string{"--url", s.url, "--user", "admin", "--output", dir}); err == nil {
		t.Fatalf("a wrong password is refused")
	}
}

func TestPeriodicBackupsKeepTheLastOnes(t *testing.T) {
	startLiveServer(t, nil)
	fillRandom(t, 10, 100)

	dir := t.TempDir()
	for _, name := range []string{"elysiandb-backup-20200101T000000Z.tar.gz", "elysiandb-backup-20200102T000000Z.tar.gz", "other.txt"} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
	}

	backup.RunPeriodic(dir, 2)

	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}

	names := []string{}
	for _, entry := range entries {
		names = append(names, entry.Name())
	}
	sort.Strings(names)

	if len(names) != 3 || names[0] != "elysiandb-backup-20200102T000000Z.tar.gz" || names[2] != "other.txt" {
		t.Fatalf("kept %v", names)
	}

	if st := backup.CurrentStatus(); !st.LastOK || st.Documents < 10 {
		t.Fatalf("status %+v", st)
	}
}

func TestWritesAreRefusedWithoutDiskAndComeBack(t *testing.T) {
	var free atomic.Int64
	free.Store(1 << 40)
	storage.FreeSpaceProbe = func() (int64, error) { return free.Load(), nil }
	t.Cleanup(func() { storage.FreeSpaceProbe = nil })

	s := startLiveServer(t, nil)
	s.login()

	if status, body := s.do("POST", "/api/full", strings.NewReader(`{"id":"before"}`), "application/json"); status != http.StatusOK {
		t.Fatalf("write: %d %s", status, body)
	}

	free.Store(1)
	deadline := time.Now().Add(5 * time.Second)
	for storage.WriteRefusal() == nil {
		if time.Now().After(deadline) {
			t.Fatal("the low disk is not noticed")
		}
		time.Sleep(20 * time.Millisecond)
	}

	status, body := s.do("POST", "/api/full", strings.NewReader(`{"id":"refused"}`), "application/json")
	if status != http.StatusInsufficientStorage || !strings.Contains(string(body), "disk full") {
		t.Fatalf("expected a refusal, got %d %s", status, body)
	}

	if status, _ := s.do("PUT", "/kv/refused", strings.NewReader("v"), ""); status != http.StatusInsufficientStorage {
		t.Fatalf("key write: %d", status)
	}

	if status, body := s.do("GET", "/api/full/before", nil, ""); status != http.StatusOK || !strings.Contains(string(body), "before") {
		t.Fatalf("reads are served: %d %s", status, body)
	}

	if status, body := s.do("POST", "/api/query", strings.NewReader(`{"entity":"full","filters":{}}`), "application/json"); status == http.StatusInsufficientStorage {
		t.Fatalf("queries are served: %d %s", status, body)
	}

	if status, _ := s.do("GET", "/api/full/refused", nil, ""); status != http.StatusNotFound {
		t.Fatalf("a refused write leaves nothing: %d", status)
	}

	if storage.RefusedWrites() < 2 {
		t.Fatalf("refusals are counted: %d", storage.RefusedWrites())
	}

	if status, _ := s.do("DELETE", "/api/full/before", nil, ""); status >= 500 {
		t.Fatalf("deleting is how space comes back: %d", status)
	}

	free.Store(1 << 40)
	deadline = time.Now().Add(5 * time.Second)
	for storage.WriteRefusal() != nil {
		if time.Now().After(deadline) {
			t.Fatal("writes never resume")
		}
		time.Sleep(20 * time.Millisecond)
	}

	if status, body := s.do("POST", "/api/full", strings.NewReader(`{"id":"after"}`), "application/json"); status != http.StatusOK {
		t.Fatalf("writes resume: %d %s", status, body)
	}
}
