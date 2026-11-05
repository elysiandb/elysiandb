package process_test

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"math/rand"
	"net"
	"net/http"
	"net/http/cookiejar"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"testing"
	"time"
)

var binary string

func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "elysiandb-process-")
	if err != nil {
		fmt.Println(err)
		os.Exit(1)
	}

	binary = filepath.Join(dir, "elysiandb")
	build := exec.Command("go", "build", "-o", binary, "../../..")
	if out, err := build.CombinedOutput(); err != nil {
		fmt.Printf("cannot build the binary: %v\n%s", err, out)
		os.Exit(1)
	}

	code := m.Run()
	os.RemoveAll(dir)
	os.Exit(code)
}

type server struct {
	t      *testing.T
	cmd    *exec.Cmd
	url    string
	client *http.Client
	folder string
	config string
	log    *bytes.Buffer
}

func freePort(t *testing.T) int {
	t.Helper()

	ln, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()

	return ln.Addr().(*net.TCPAddr).Port
}

func writeConfig(t *testing.T, folder string, port int, extra string) string {
	t.Helper()

	config := fmt.Sprintf(`store:
  folder: %s
  shards: 16
  flushIntervalSeconds: 1
  crashRecovery: { enabled: true, maxLogMB: 1 }
%s
engine:
  name: internal
server:
  http: { enabled: true, host: 127.0.0.1, port: %d }
  tcp: { enabled: false, host: 127.0.0.1, port: 1 }
security:
  authentication: { enabled: true, mode: user }
api:
  schema: { enabled: false }
  hooks: { enabled: true }
log: { level: info, format: text }
`, folder, extra, port)

	path := folder + ".yaml"
	if err := os.WriteFile(path, []byte(config), 0o600); err != nil {
		t.Fatal(err)
	}

	return path
}

func start(t *testing.T, folder, extra string) *server {
	t.Helper()

	port := freePort(t)
	config := writeConfig(t, folder, port, extra)
	output := &bytes.Buffer{}

	cmd := exec.Command(binary, "--config", config)
	cmd.Stdout = output
	cmd.Stderr = output
	cmd.Env = append(os.Environ(), "ELYSIANDB_ADMIN_PASSWORD=process-test-password")
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}

	jar, _ := cookiejar.New(nil)
	s := &server{t: t, cmd: cmd, url: fmt.Sprintf("http://127.0.0.1:%d", port), folder: folder, config: config, log: output, client: &http.Client{Jar: jar, Timeout: 30 * time.Second, Transport: &http.Transport{MaxIdleConnsPerHost: 64}}}

	deadline := time.Now().Add(60 * time.Second)
	for {
		resp, err := s.client.Get(s.url + "/health")
		if err == nil {
			resp.Body.Close()
			if resp.StatusCode == http.StatusOK {
				break
			}
		}
		if time.Now().After(deadline) {
			_ = cmd.Process.Kill()
			t.Fatalf("the server never became ready:\n%s", output.String())
		}
		time.Sleep(20 * time.Millisecond)
	}

	resp, err := s.client.Post(s.url+"/api/security/login", "application/json", strings.NewReader(`{"username":"admin","password":"process-test-password"}`))
	if err != nil {
		t.Fatal(err)
	}
	resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("login: %s\n%s", resp.Status, output.String())
	}

	return s
}

func (s *server) kill() {
	_ = s.cmd.Process.Signal(syscall.SIGKILL)
	_ = s.cmd.Wait()
}

func (s *server) terminate() time.Duration {
	begin := time.Now()
	_ = s.cmd.Process.Signal(syscall.SIGTERM)
	done := make(chan struct{})
	go func() {
		_ = s.cmd.Wait()
		close(done)
	}()

	select {
	case <-done:
	case <-time.After(30 * time.Second):
		_ = s.cmd.Process.Kill()
		s.t.Fatalf("the server does not stop:\n%s", s.log.String())
	}

	return time.Since(begin)
}

func (s *server) do(method, path, body string) (int, []byte, error) {
	req, err := http.NewRequest(method, s.url+path, strings.NewReader(body))
	if err != nil {
		return 0, nil, err
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := s.client.Do(req)
	if err != nil {
		return 0, nil, err
	}
	defer resp.Body.Close()

	data, err := io.ReadAll(resp.Body)

	return resp.StatusCode, data, err
}

type confirmed struct {
	mu      sync.Mutex
	docs    map[string]int
	deleted map[string]bool
	kv      map[string]int
	bulk    map[string]bool
}

func newConfirmed() *confirmed {
	return &confirmed{docs: map[string]int{}, deleted: map[string]bool{}, kv: map[string]int{}, bulk: map[string]bool{}}
}

func writeLoad(s *server, c *confirmed, round int, stop *atomic.Bool, wg *sync.WaitGroup) {
	for w := 0; w < 4; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			for n := 0; !stop.Load(); n++ {
				id := fmt.Sprintf("r%d-w%d-%d", round, w, n%40)
				body := fmt.Sprintf(`{"id":%q,"round":%d,"n":%d,"pad":%q}`, id, round, n, strings.Repeat("p", 300))
				status, _, err := s.do("PUT", "/api/single/"+id, body)
				if err != nil {
					return
				}
				if status == http.StatusNotFound {
					status, _, err = s.do("POST", "/api/single", body)
					if err != nil {
						return
					}
				}
				if status == http.StatusOK || status == http.StatusCreated {
					c.mu.Lock()
					c.docs[id] = n
					c.mu.Unlock()
				}
			}
		}(w)
	}

	wg.Add(1)
	go func() {
		defer wg.Done()
		for b := 0; !stop.Load(); b++ {
			docs := make([]string, 0, 200)
			ids := make([]string, 0, 200)
			for i := 0; i < 200; i++ {
				id := fmt.Sprintf("bulk-r%d-%d-%d", round, b, i)
				ids = append(ids, id)
				docs = append(docs, fmt.Sprintf(`{"id":%q,"round":%d,"pad":%q}`, id, round, strings.Repeat("b", 200)))
			}
			status, _, err := s.do("POST", "/api/bulk", "["+strings.Join(docs, ",")+"]")
			if err != nil {
				return
			}
			if status == http.StatusOK || status == http.StatusCreated {
				c.mu.Lock()
				for _, id := range ids {
					c.bulk[id] = true
				}
				c.mu.Unlock()
			}
		}
	}()

	wg.Add(1)
	go func() {
		defer wg.Done()
		for n := 0; !stop.Load(); n++ {
			key := fmt.Sprintf("k%d-%d", round, n%10)
			status, _, err := s.do("PUT", "/kv/"+key, fmt.Sprintf("%d", n))
			if err != nil {
				return
			}
			if status == http.StatusNoContent {
				c.mu.Lock()
				c.kv[key] = n
				c.mu.Unlock()
			}
		}
	}()

	wg.Add(1)
	go func() {
		defer wg.Done()
		for n := 0; !stop.Load(); n++ {
			id := fmt.Sprintf("gone-r%d-%d", round, n)
			status, _, err := s.do("POST", "/api/gone", fmt.Sprintf(`{"id":%q}`, id))
			if err != nil || (status != http.StatusOK && status != http.StatusCreated) {
				return
			}
			status, _, err = s.do("DELETE", "/api/gone/"+id, "")
			if err != nil {
				return
			}
			if status < 300 {
				c.mu.Lock()
				c.deleted[id] = true
				c.mu.Unlock()
			}
		}
	}()

	wg.Add(1)
	go func() {
		defer wg.Done()
		for n := 0; !stop.Load(); n++ {
			if _, _, err := s.do("POST", "/api/junk", fmt.Sprintf(`{"id":"j%d","pad":%q}`, n, strings.Repeat("j", 400))); err != nil {
				return
			}
			if n%50 == 49 {
				if _, _, err := s.do("DELETE", "/api/junk", ""); err != nil {
					return
				}
			}
		}
	}()
}

func verifyConfirmed(t *testing.T, s *server, c *confirmed, label string) {
	t.Helper()

	c.mu.Lock()
	defer c.mu.Unlock()

	for id, n := range c.docs {
		status, body, err := s.do("GET", "/api/single/"+id, "")
		if err != nil || status != http.StatusOK {
			t.Fatalf("%s: confirmed document %s is lost: %d %v\n%s", label, id, status, err, s.log.String())
		}

		var doc map[string]any
		if err := json.Unmarshal(body, &doc); err != nil {
			t.Fatalf("%s: document %s is corrupt: %v", label, id, err)
		}

		if got, _ := doc["n"].(float64); int(got) < n {
			t.Fatalf("%s: document %s went back to version %v, %d was confirmed", label, id, got, n)
		}
	}

	if len(c.bulk) > 0 {
		status, body, err := s.do("GET", "/api/bulk?fields=id", "")
		if err != nil || status != http.StatusOK {
			t.Fatalf("%s: cannot list the bulk documents: %d %v", label, status, err)
		}

		var listed []map[string]any
		if err := json.Unmarshal(body, &listed); err != nil {
			t.Fatalf("%s: the list is corrupt: %v", label, err)
		}

		present := make(map[string]bool, len(listed))
		for _, doc := range listed {
			id, _ := doc["id"].(string)
			present[id] = true
		}

		for id := range c.bulk {
			if !present[id] {
				t.Fatalf("%s: confirmed bulk document %s is lost", label, id)
			}
		}
	}

	for id := range c.deleted {
		status, _, err := s.do("GET", "/api/gone/"+id, "")
		if err != nil || status != http.StatusNotFound {
			t.Fatalf("%s: confirmed delete of %s is lost: %d %v", label, id, status, err)
		}
	}

	for key, n := range c.kv {
		status, body, err := s.do("GET", "/kv/"+key, "")
		if err != nil || status != http.StatusOK {
			t.Fatalf("%s: confirmed key %s is lost: %d %v", label, key, status, err)
		}

		var kv struct {
			Value string `json:"value"`
		}
		_ = json.Unmarshal(body, &kv)
		var got int
		_, _ = fmt.Sscanf(kv.Value, "%d", &got)
		if got < n {
			t.Fatalf("%s: key %s went back to %d, %d was confirmed", label, key, got, n)
		}
	}
}

func killRounds(t *testing.T, extra string) {
	if testing.Short() {
		t.Skip("spawns servers")
	}

	folder := filepath.Join(t.TempDir(), "data")
	c := newConfirmed()
	r := rand.New(rand.NewSource(int64(os.Getpid())))

	for round := 0; round < 6; round++ {
		s := start(t, folder, extra)
		verifyConfirmed(t, s, c, fmt.Sprintf("start of round %d", round))

		var stop atomic.Bool
		var wg sync.WaitGroup
		writeLoad(s, c, round, &stop, &wg)

		if round%2 == 1 {
			wg.Add(1)
			go func() {
				defer wg.Done()
				resp, err := s.client.Get(s.url + "/api/backup")
				if err == nil {
					_, _ = io.Copy(io.Discard, resp.Body)
					resp.Body.Close()
				}
			}()
		}

		time.Sleep(time.Duration(300+r.Intn(1200)) * time.Millisecond)
		s.kill()
		stop.Store(true)
		wg.Wait()
	}

	s := start(t, folder, extra)
	verifyConfirmed(t, s, c, "after the last kill")

	if took := s.terminate(); took > 9*time.Second {
		t.Fatalf("the server should stop before docker kills it, took %s", took)
	}

	if !strings.Contains(s.log.String(), "shutdown complete") {
		t.Fatalf("no clean shutdown:\n%s", s.log.String())
	}

	s = start(t, folder, extra)
	verifyConfirmed(t, s, c, "after the clean stop")
	s.terminate()

	logs := s.log.String()
	if strings.Contains(logs, "level=ERROR") {
		t.Fatalf("errors in the logs:\n%s", logs)
	}
}

func TestKillAtAnyMomentLosesNoConfirmedWrite(t *testing.T) {
	killRounds(t, "")
}

func TestKillWithSyncedWritesLosesNoConfirmedWrite(t *testing.T) {
	killRounds(t, "  durability: fsync")
}

func TestAStoppedServerUnderLoadLeavesInTime(t *testing.T) {
	if testing.Short() {
		t.Skip("spawns servers")
	}

	folder := filepath.Join(t.TempDir(), "data")
	s := start(t, folder, "")
	c := newConfirmed()

	var stop atomic.Bool
	var wg sync.WaitGroup
	writeLoad(s, c, 0, &stop, &wg)
	time.Sleep(1500 * time.Millisecond)

	took := s.terminate()
	stop.Store(true)
	wg.Wait()

	if took > 9*time.Second {
		t.Fatalf("docker kills after 10 seconds, the server took %s", took)
	}

	s = start(t, folder, "")
	verifyConfirmed(t, s, c, "after the stop")
	s.terminate()
}

func TestAnInterruptedRestoreIsReportedAndCanBeRunAgain(t *testing.T) {
	if testing.Short() {
		t.Skip("spawns servers")
	}

	folder := filepath.Join(t.TempDir(), "data")
	s := start(t, folder, "")
	docs := make([]string, 0, 500)
	for i := 0; i < 500; i++ {
		docs = append(docs, fmt.Sprintf(`{"id":"d%d","pad":%q}`, i, strings.Repeat("r", 500)))
	}
	for b := 0; b < 20; b++ {
		for i := range docs {
			docs[i] = fmt.Sprintf(`{"id":"d%d-%d","pad":%q}`, b, i, strings.Repeat("r", 500))
		}
		if status, body, err := s.do("POST", "/api/many", "["+strings.Join(docs, ",")+"]"); err != nil || status >= 300 {
			t.Fatalf("fill: %d %s %v", status, body, err)
		}
	}

	status, archive, err := s.do("GET", "/api/backup", "")
	if err != nil || status != http.StatusOK {
		t.Fatalf("backup: %d %v", status, err)
	}

	go func() {
		req, _ := http.NewRequest("POST", s.url+"/api/restore", bytes.NewReader(archive))
		req.Header.Set("Content-Type", "application/gzip")
		resp, err := s.client.Do(req)
		if err == nil {
			resp.Body.Close()
		}
	}()

	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(filepath.Join(folder, "RESTORING")); err == nil {
			break
		}
		time.Sleep(time.Millisecond)
	}
	s.kill()

	interrupted := false
	if _, err := os.Stat(filepath.Join(folder, "RESTORING")); err == nil {
		interrupted = true
	}

	s = start(t, folder, "")
	if interrupted && !strings.Contains(s.log.String(), "a restore was interrupted") {
		t.Fatalf("an interrupted restore is reported:\n%s", s.log.String())
	}

	left, _ := filepath.Glob(filepath.Join(folder, ".restore-upload-*"))
	if len(left) != 0 {
		t.Fatalf("abandoned uploads are removed: %v", left)
	}

	req, _ := http.NewRequest("POST", s.url+"/api/restore", bytes.NewReader(archive))
	req.Header.Set("Content-Type", "application/gzip")
	resp, err := s.client.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	report, _ := io.ReadAll(resp.Body)
	resp.Body.Close()
	if resp.StatusCode != http.StatusOK || !strings.Contains(string(report), `"many":10000`) {
		t.Fatalf("restore: %s %s", resp.Status, report)
	}

	if _, err := os.Stat(filepath.Join(folder, "RESTORING")); err == nil {
		t.Fatalf("a finished restore leaves no mark")
	}
	s.terminate()
}
