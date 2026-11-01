package log_test

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"os/exec"
	"strings"
	"sync"
	"testing"
	"time"

	pkglog "github.com/taymour/elysiandb/internal/log"
)

type safeBuffer struct {
	mu sync.Mutex
	b  bytes.Buffer
}

func (s *safeBuffer) Write(p []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	return s.b.Write(p)
}

func (s *safeBuffer) String() string {
	s.mu.Lock()
	defer s.mu.Unlock()

	return s.b.String()
}

func setup(t *testing.T, opts pkglog.Options) *safeBuffer {
	t.Helper()

	if err := pkglog.Configure(opts); err != nil {
		t.Fatalf("configure: %v", err)
	}

	buf := &safeBuffer{}
	pkglog.SetOutput(buf, false)

	t.Cleanup(func() {
		pkglog.SetOutput(nil, false)
		_ = pkglog.Configure(pkglog.Options{})
	})

	return buf
}

func lines(s string) []string {
	return strings.Split(strings.TrimRight(s, "\n"), "\n")
}

func TestTextLineIsWrittenImmediatelyWithAttributes(t *testing.T) {
	buf := setup(t, pkglog.Options{})

	pkglog.For("storage").Info("storage loaded", "folder", "/data", "kv_keys", 12, "duration_ms", 1.5)

	out := buf.String()
	for _, want := range []string{"level=INFO", `msg="storage loaded"`, "component=storage", "folder=/data", "kv_keys=12", "duration_ms=1.5"} {
		if !strings.Contains(out, want) {
			t.Fatalf("missing %q in %q", want, out)
		}
	}

	if !strings.HasPrefix(out, "time=") || !strings.HasSuffix(out, "\n") {
		t.Fatalf("unexpected line %q", out)
	}
}

func TestComponentComesBeforeRecordAttributes(t *testing.T) {
	buf := setup(t, pkglog.Options{})

	pkglog.For("http").Error("request failed", "status", 500)

	out := buf.String()
	if strings.Index(out, "component=http") > strings.Index(out, "status=500") {
		t.Fatalf("component should come first: %q", out)
	}
}

func TestClientValuesAreQuotedAndEscaped(t *testing.T) {
	buf := setup(t, pkglog.Options{})

	pkglog.Warn("unknown command", "command", "FOO\x1b[31m bar\nlevel=ERROR")

	out := buf.String()
	if strings.Count(out, "\n") != 1 {
		t.Fatalf("a client value must not split the line: %q", out)
	}
	if strings.Contains(out, "\x1b") {
		t.Fatalf("escape sequences must be escaped: %q", out)
	}
	if !strings.Contains(out, `command="FOO\x1b[31m bar\nlevel=ERROR"`) {
		t.Fatalf("unexpected quoting: %q", out)
	}
}

func TestJSONFormat(t *testing.T) {
	buf := setup(t, pkglog.Options{Format: pkglog.FormatJSON})

	pkglog.For("security").Warn("login failed", "user", "admin", "attempts", 3, "err", errors.New("bad password"))

	var line map[string]any
	if err := json.Unmarshal([]byte(buf.String()), &line); err != nil {
		t.Fatalf("not JSON: %v %q", err, buf.String())
	}

	want := map[string]any{"level": "WARN", "msg": "login failed", "component": "security", "user": "admin", "attempts": float64(3), "err": "bad password"}
	for k, v := range want {
		if line[k] != v {
			t.Fatalf("%s = %v, want %v (%v)", k, line[k], v, line)
		}
	}

	if _, err := time.Parse(time.RFC3339Nano, line["time"].(string)); err != nil {
		t.Fatalf("time: %v", err)
	}
}

func TestLevelFiltering(t *testing.T) {
	buf := setup(t, pkglog.Options{Level: "warn"})

	pkglog.Debug("d")
	pkglog.Info("i")
	pkglog.Warn("w")
	pkglog.Error("e")

	got := lines(buf.String())
	if len(got) != 2 || !strings.Contains(got[0], "msg=w") || !strings.Contains(got[1], "msg=e") {
		t.Fatalf("unexpected lines %q", got)
	}

	if pkglog.Enabled(slog.LevelInfo) || !pkglog.Enabled(slog.LevelWarn) {
		t.Fatalf("Enabled does not follow the level")
	}
}

func TestLinesKeepTheirOrder(t *testing.T) {
	buf := setup(t, pkglog.Options{})

	for i := 0; i < 500; i++ {
		pkglog.Info("step", "i", i)
	}

	for i, line := range lines(buf.String()) {
		if !strings.HasSuffix(line, fmt.Sprintf(" i=%d", i)) {
			t.Fatalf("line %d out of order: %q", i, line)
		}
	}
}

func TestConcurrentLoggingIsSafe(t *testing.T) {
	buf := setup(t, pkglog.Options{BufferLines: 100})

	var wg sync.WaitGroup
	for g := 0; g < 8; g++ {
		wg.Add(1)
		go func(g int) {
			defer wg.Done()
			for i := 0; i < 200; i++ {
				pkglog.Info("concurrent", "g", g, "i", i)
				_ = pkglog.Logs(pkglog.Query{Limit: 10})
			}
		}(g)
	}
	wg.Wait()

	if got := len(lines(buf.String())); got != 1600 {
		t.Fatalf("expected 1600 lines, got %d", got)
	}
}

func TestConfigureRejectsUnknownValues(t *testing.T) {
	if err := pkglog.Configure(pkglog.Options{Level: "verbose"}); err == nil {
		t.Fatalf("expected an error for an unknown level")
	}
	if err := pkglog.Configure(pkglog.Options{Format: "xml"}); err == nil {
		t.Fatalf("expected an error for an unknown format")
	}
	_ = pkglog.Configure(pkglog.Options{})
}

func TestParseLevel(t *testing.T) {
	cases := map[string]slog.Level{"": slog.LevelInfo, "INFO": slog.LevelInfo, "debug": slog.LevelDebug, "warning": slog.LevelWarn, " error ": slog.LevelError}
	for in, want := range cases {
		got, err := pkglog.ParseLevel(in)
		if err != nil || got != want {
			t.Fatalf("ParseLevel(%q) = %v %v", in, got, err)
		}
	}
}

func TestCountsWarningsAndErrors(t *testing.T) {
	setup(t, pkglog.Options{})

	warnings, errorsCount := pkglog.Counts()
	pkglog.Warn("w")
	pkglog.Error("e")
	pkglog.Error("e")
	pkglog.Info("i")

	w, e := pkglog.Counts()
	if w-warnings != 1 || e-errorsCount != 2 {
		t.Fatalf("unexpected counts %d %d", w-warnings, e-errorsCount)
	}
}

func TestBufferIsDisabledWithZeroLines(t *testing.T) {
	setup(t, pkglog.Options{BufferLines: 0})

	pkglog.Info("not kept")

	if pkglog.BufferEnabled() {
		t.Fatalf("buffer should be disabled")
	}
	if page := pkglog.Logs(pkglog.Query{}); len(page.Entries) != 0 {
		t.Fatalf("unexpected entries %v", page.Entries)
	}
}

func TestBufferKeepsTheLastLinesNewestFirst(t *testing.T) {
	setup(t, pkglog.Options{BufferLines: 3})

	for i := 0; i < 5; i++ {
		pkglog.For("boot").Info("line", "i", i)
	}

	page := pkglog.Logs(pkglog.Query{})
	if page.Size != 3 || page.Capacity != 3 || len(page.Entries) != 3 {
		t.Fatalf("unexpected page %+v", page)
	}

	if page.Entries[0].Attrs["i"] != int64(4) || page.Entries[2].Attrs["i"] != int64(2) {
		t.Fatalf("unexpected order %+v", page.Entries)
	}

	if page.Entries[0].Component != "boot" || page.Entries[0].Level != "INFO" {
		t.Fatalf("unexpected entry %+v", page.Entries[0])
	}

	if page.OldestSeq != page.Entries[2].Seq || page.LatestSeq != page.Entries[0].Seq || page.BufferSince == nil {
		t.Fatalf("unexpected bounds %+v", page)
	}
}

func TestBufferQueries(t *testing.T) {
	setup(t, pkglog.Options{BufferLines: 100})

	pkglog.For("http").Info("request", "route", "/api/{entity}", "request_id", "r-1", "user", "alice")
	pkglog.For("http").Error("request failed", "route", "/api/{entity}", "request_id", "r-2", "err", "Timeout reached")
	pkglog.For("security").Warn("login failed", "user", "bob")
	pkglog.For("audit").Info("user created", "actor", "alice", "user", "carol")
	pkglog.For("storage").Debug("flush done")

	count := func(q pkglog.Query) int { return len(pkglog.Logs(q).Entries) }

	if got := count(pkglog.Query{MinLevel: slog.LevelWarn}); got != 2 {
		t.Fatalf("min level: %d", got)
	}
	if got := count(pkglog.Query{Component: "http"}); got != 2 {
		t.Fatalf("component: %d", got)
	}
	if got := count(pkglog.Query{Text: "timeout"}); got != 1 {
		t.Fatalf("text in attributes: %d", got)
	}
	if got := count(pkglog.Query{Text: "LOGIN"}); got != 1 {
		t.Fatalf("text in message: %d", got)
	}
	if got := count(pkglog.Query{User: "alice"}); got != 2 {
		t.Fatalf("user or actor: %d", got)
	}
	if got := count(pkglog.Query{RequestID: "r-2"}); got != 1 {
		t.Fatalf("request id: %d", got)
	}

	all := pkglog.Logs(pkglog.Query{})
	if strings.Join(all.Components, ",") != "audit,http,security" {
		t.Fatalf("debug lines below the level are not kept, components %v", all.Components)
	}

	newest := all.Entries[0].Seq
	pkglog.Info("after")
	if after := pkglog.Logs(pkglog.Query{After: newest}); len(after.Entries) != 1 || after.Entries[0].Message != "after" {
		t.Fatalf("after: %+v", after.Entries)
	}

	if before := pkglog.Logs(pkglog.Query{Before: newest}); len(before.Entries) != 3 {
		t.Fatalf("before: %d", len(before.Entries))
	}

	page := pkglog.Logs(pkglog.Query{Limit: 2})
	if len(page.Entries) != 2 || !page.HasMore {
		t.Fatalf("limit: %+v", page)
	}

	since := pkglog.Logs(pkglog.Query{Since: time.Now().Add(time.Hour)})
	if len(since.Entries) != 0 {
		t.Fatalf("since: %d", len(since.Entries))
	}
}

func TestBufferResizeKeepsTheNewestLines(t *testing.T) {
	setup(t, pkglog.Options{BufferLines: 10})

	for i := 0; i < 10; i++ {
		pkglog.Info("line", "i", i)
	}

	_ = pkglog.Configure(pkglog.Options{BufferLines: 4})

	page := pkglog.Logs(pkglog.Query{})
	if len(page.Entries) != 4 || page.Entries[0].Attrs["i"] != int64(9) || page.Entries[3].Attrs["i"] != int64(6) {
		t.Fatalf("unexpected entries %+v", page.Entries)
	}
}

func TestTemporaryLevelGoesBackByItself(t *testing.T) {
	buf := setup(t, pkglog.Options{Level: "info", BufferLines: 10})

	info := pkglog.SetLevelFor(slog.LevelDebug, 50*time.Millisecond)
	if info.Level != "debug" || info.Configured != "info" || info.Until == nil {
		t.Fatalf("unexpected info %+v", info)
	}

	pkglog.Debug("visible")

	deadline := time.Now().Add(2 * time.Second)
	for pkglog.CurrentLevel().Level != "info" && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}

	if current := pkglog.CurrentLevel(); current.Level != "info" || current.Until != nil {
		t.Fatalf("level not restored: %+v", current)
	}

	pkglog.Debug("hidden")

	out := buf.String()
	if !strings.Contains(out, "msg=visible") || strings.Contains(out, "msg=hidden") {
		t.Fatalf("unexpected output %q", out)
	}
	if !strings.Contains(out, `msg="log level restored"`) {
		t.Fatalf("the restore should be logged: %q", out)
	}
}

func TestSettingTheConfiguredLevelCancelsTheOverride(t *testing.T) {
	setup(t, pkglog.Options{Level: "warn"})

	pkglog.SetLevelFor(slog.LevelDebug, time.Hour)
	info := pkglog.SetLevelFor(slog.LevelWarn, 0)

	if info.Level != "warn" || info.Until != nil {
		t.Fatalf("unexpected info %+v", info)
	}
}

func TestThrottle(t *testing.T) {
	th := pkglog.NewThrottle(time.Minute)
	now := time.Unix(1_000, 0)
	th.SetClock(func() time.Time { return now })

	if n, ok := th.Allow("k"); !ok || n != 0 {
		t.Fatalf("first call should pass")
	}

	for i := 0; i < 3; i++ {
		if _, ok := th.Allow("k"); ok {
			t.Fatalf("call %d should be suppressed", i)
		}
	}

	if _, ok := th.Allow("other"); !ok {
		t.Fatalf("keys are independent")
	}

	now = now.Add(time.Minute)
	if n, ok := th.Allow("k"); !ok || n != 3 {
		t.Fatalf("expected 3 suppressed, got %d %v", n, ok)
	}

	th.Allow("k")
	if n := th.Reset("k"); n != 1 {
		t.Fatalf("reset should return the suppressed count, got %d", n)
	}
	if _, ok := th.Allow("k"); !ok {
		t.Fatalf("a reset key passes again")
	}
}

func TestWithSuppressed(t *testing.T) {
	if got := pkglog.WithSuppressed([]any{"a", 1}, 0); len(got) != 2 {
		t.Fatalf("no attribute expected for 0: %v", got)
	}
	if got := pkglog.WithSuppressed([]any{"a", 1}, 4); len(got) != 4 || got[2] != "suppressed" || got[3] != 4 {
		t.Fatalf("unexpected %v", got)
	}
}

func TestFatalLogsAndExitsWithCodeOne(t *testing.T) {
	if os.Getenv("RUN_FATAL_HELPER") == "1" {
		pkglog.Fatal("boom", "err", fmt.Errorf("kaboom"))
		return
	}

	cmd := exec.Command(os.Args[0], "-test.run", t.Name())
	cmd.Env = append(os.Environ(), "RUN_FATAL_HELPER=1")
	out, err := cmd.CombinedOutput()

	var exitErr *exec.ExitError
	if !errors.As(err, &exitErr) || exitErr.ExitCode() != 1 {
		t.Fatalf("expected exit code 1, got %v", err)
	}

	if !strings.Contains(string(out), "level=ERROR") || !strings.Contains(string(out), "msg=boom") || !strings.Contains(string(out), "err=kaboom") {
		t.Fatalf("the fatal line should be written: %q", out)
	}
}
