package main

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("http", run)
}

type step struct {
	data  string
	pause int
}

type readSpec struct {
	Until   string `json:"until"`
	QuietMs int    `json:"quiet_ms,omitempty"`
	MaxMs   int    `json:"max_ms"`
}

type tcase struct {
	slug      string
	conn      string
	steps     []step
	halfClose bool
	read      readSpec
	compare   string
	capture   map[string]string
	head      bool
	server    string
}

type scenario struct {
	name   string
	config string
	seed   func(addr string)
	cases  func() []tcase
	before func()
	after  func()
}

type connLogger struct {
	n atomic.Int64
}

func (l *connLogger) Printf(format string, args ...any) {
	l.n.Add(1)
}

var tmpBase string

func run(out string) error {
	base, err := os.MkdirTemp("", "elysiandb-golden-http-")
	if err != nil {
		return err
	}
	tmpBase = base
	defer os.RemoveAll(base)
	log.SetOutput(io.Discard, false)
	globals.VERSION = "golden"
	for _, sc := range scenarios() {
		fmt.Fprintf(os.Stderr, "http scenario %s\n", sc.name)
		if err := runScenario(out, sc); err != nil {
			return fmt.Errorf("%s: %w", sc.name, err)
		}
	}
	return nil
}

func runScenario(out string, sc scenario) error {
	data := filepath.Join(tmpBase, sc.name)
	if err := os.MkdirAll(data, 0o755); err != nil {
		return err
	}
	cfgText := strings.ReplaceAll(sc.config, "<DATA>", data)
	cfgPath := filepath.Join(tmpBase, sc.name+".yaml")
	if err := os.WriteFile(cfgPath, []byte(cfgText), 0o644); err != nil {
		return err
	}
	if err := goldenio.WriteFile(out, sc.name+"/config.yaml", []byte(sc.config)); err != nil {
		return err
	}
	cfg, err := configuration.LoadConfig(cfgPath)
	if err != nil {
		return err
	}
	if err := log.Configure(log.Options{Level: cfg.Log.Level, Format: cfg.Log.Format, BufferLines: cfg.Log.BufferLines}); err != nil {
		return err
	}
	log.SetOutput(io.Discard, false)
	globals.SetConfig(cfg)
	security.ResetLoginAttempts()
	if sc.before != nil {
		sc.before()
	}
	boot.InitDB()
	if err := security.InitAdminUserIfNotExists(); err != nil {
		return err
	}
	security.ProtectSessionsFile()
	if cfg.Stats.Enabled {
		boot.BootStats()
	}
	boot.BootMonitoring()
	globals.SetStarting(false)
	logger := &connLogger{}
	srv := boot.NewHTTPServer()
	srv.Logger = logger
	ln, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		return err
	}
	go func() { _ = srv.Serve(ln) }()
	limited := boot.NewHTTPServer()
	limited.Concurrency = 1
	limited.Logger = logger
	ln2, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		return err
	}
	go func() { _ = limited.Serve(ln2) }()
	addrs := map[string]string{"": ln.Addr().String(), "limited": ln2.Addr().String()}
	if sc.seed != nil {
		sc.seed(addrs[""])
	}
	norm := newNormalizer(data)
	captured := map[string]string{}
	conns := map[string]net.Conn{}
	var entries []map[string]any
	for i, c := range sc.cases() {
		id := fmt.Sprintf("%03d-%s", i+1, c.slug)
		var conn net.Conn
		if strings.HasPrefix(c.conn, "reuse:") {
			conn = conns[strings.TrimPrefix(c.conn, "reuse:")]
		} else {
			conn, err = net.Dial("tcp4", addrs[c.server])
			if err != nil {
				return err
			}
		}
		conns[id] = conn
		before := logger.n.Load()
		var files []map[string]any
		for si, st := range c.steps {
			name := id + ".req"
			if si > 0 {
				name = fmt.Sprintf("%s.%d.req", id, si+1)
			}
			if err := goldenio.WriteFile(out, sc.name+"/"+name, []byte(st.data)); err != nil {
				return err
			}
			files = append(files, map[string]any{"file": name, "pause_before_ms": st.pause})
			if st.pause > 0 {
				time.Sleep(time.Duration(st.pause) * time.Millisecond)
			}
			payload := st.data
			for k, v := range captured {
				payload = strings.ReplaceAll(payload, "{{"+k+"}}", v)
			}
			_, _ = conn.Write([]byte(payload))
		}
		if c.halfClose {
			if tc, ok := conn.(*net.TCPConn); ok {
				_ = tc.CloseWrite()
			}
		}
		raw, closed := readResponse(conn, c.read)
		time.Sleep(20 * time.Millisecond)
		logged := logger.n.Load() > before
		for k, pattern := range c.capture {
			if m := regexp.MustCompile(pattern).FindSubmatch(raw); m != nil {
				captured[k] = string(m[1])
			}
		}
		if c.compare != "head-only" && serverOwnError(raw) {
			c.compare = "d27"
		}
		normalized := norm.normalize(raw, c.head, c.compare)
		if err := goldenio.WriteFile(out, sc.name+"/"+id+".resp", normalized); err != nil {
			return err
		}
		connMode := "new"
		if c.conn != "" {
			connMode = c.conn
		}
		entry := map[string]any{"id": id, "conn": connMode, "steps": files, "half_close": c.halfClose, "read": c.read, "closed": closed, "logged": logged, "compare": c.compare}
		if c.server != "" {
			entry["server"] = c.server
		}
		if c.head {
			entry["head"] = true
		}
		if len(c.capture) > 0 {
			entry["capture"] = c.capture
		}
		entries = append(entries, entry)
	}
	for _, c := range conns {
		_ = c.Close()
	}
	_ = srv.Shutdown()
	_ = limited.Shutdown()
	if sc.after != nil {
		sc.after()
	}
	_ = storage.Close()
	data2, err := goldenio.MarshalIndent(entries)
	if err != nil {
		return err
	}
	return goldenio.WriteFile(out, sc.name+"/cases.json", data2)
}

func readResponse(conn net.Conn, rs readSpec) ([]byte, bool) {
	var buf bytes.Buffer
	deadline := time.Now().Add(time.Duration(rs.MaxMs) * time.Millisecond)
	quiet := time.Duration(rs.QuietMs) * time.Millisecond
	if rs.Until == "quiet" && quiet == 0 {
		quiet = 800 * time.Millisecond
	}
	chunk := make([]byte, 64<<10)
	for {
		wait := time.Until(deadline)
		if wait <= 0 {
			return buf.Bytes(), false
		}
		if rs.Until == "quiet" && quiet < wait {
			wait = quiet
		}
		_ = conn.SetReadDeadline(time.Now().Add(wait))
		n, err := conn.Read(chunk)
		buf.Write(chunk[:n])
		if err != nil {
			if ne, ok := err.(net.Error); ok && ne.Timeout() {
				if rs.Until == "quiet" {
					return buf.Bytes(), false
				}
				if time.Now().After(deadline) {
					return buf.Bytes(), false
				}
				continue
			}
			return buf.Bytes(), true
		}
	}
}

type normalizer struct {
	data  string
	tmp   string
	uuids map[string]int
	txids map[string]int
	mu    sync.Mutex
}

func newNormalizer(data string) *normalizer {
	return &normalizer{data: data, tmp: os.TempDir(), uuids: map[string]int{}, txids: map[string]int{}}
}

var (
	ridPattern     = regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-z]+$`)
	uuidRe         = regexp.MustCompile(`[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`)
	txRe           = regexp.MustCompile(`\d{14}\.\d{9}`)
	timeRe         = regexp.MustCompile(`\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})`)
	stampRe        = regexp.MustCompile(`\d{8}-\d{6}|\d{8}T\d{6}Z?`)
	sessionRe      = regexp.MustCompile(`edb_session=[0-9a-f]+`)
	statsVersionRe = regexp.MustCompile(`"version":"[^"]*"`)
)

func (n *normalizer) replaceBody(body []byte) []byte {
	s := string(body)
	s = strings.ReplaceAll(s, n.data, "<DATA>")
	s = strings.ReplaceAll(s, strings.TrimSuffix(n.tmp, "/"), "<TMP>")
	s = uuidRe.ReplaceAllStringFunc(s, func(u string) string {
		if _, ok := n.uuids[u]; !ok {
			n.uuids[u] = len(n.uuids) + 1
		}
		return fmt.Sprintf("<UUID:%d>", n.uuids[u])
	})
	s = txRe.ReplaceAllStringFunc(s, func(u string) string {
		if _, ok := n.txids[u]; !ok {
			n.txids[u] = len(n.txids) + 1
		}
		return fmt.Sprintf("<TXID:%d>", n.txids[u])
	})
	s = timeRe.ReplaceAllString(s, "<TIME>")
	s = sessionRe.ReplaceAllString(s, "edb_session=<SID>")
	s = statsVersionRe.ReplaceAllString(s, `"version":"<VER>"`)
	return []byte(s)
}

func (n *normalizer) normalize(raw []byte, head bool, compare string) []byte {
	var out bytes.Buffer
	rest := raw
	for len(rest) > 0 {
		end := bytes.Index(rest, []byte("\r\n\r\n"))
		if end < 0 || !bytes.HasPrefix(rest, []byte("HTTP/")) {
			out.WriteString("<UNPARSED>\r\n")
			out.Write(n.replaceBody(rest))
			break
		}
		headText := string(rest[:end])
		rest = rest[end+4:]
		lines := strings.Split(headText, "\r\n")
		status := lines[0]
		code := 0
		if parts := strings.SplitN(status, " ", 3); len(parts) >= 2 {
			code, _ = strconv.Atoi(parts[1])
		}
		var headers [][2]string
		length := -1
		chunked := false
		for _, l := range lines[1:] {
			k, v, _ := strings.Cut(l, ":")
			v = strings.TrimSpace(v)
			headers = append(headers, [2]string{k, v})
			switch strings.ToLower(k) {
			case "content-length":
				length, _ = strconv.Atoi(v)
			case "transfer-encoding":
				chunked = strings.EqualFold(v, "chunked")
			}
		}
		var body []byte
		bodyMarker := ""
		noBody := head || code == 100 || code == 204 || code == 304
		switch {
		case noBody:
		case chunked:
			decoded, consumed, complete := decodeChunked(rest)
			body = decoded
			rest = rest[consumed:]
			if complete {
				bodyMarker = "<CHUNKED complete>\r\n"
			} else {
				bodyMarker = "<CHUNKED truncated>\r\n"
			}
		case length >= 0:
			if length > len(rest) {
				length = len(rest)
			}
			body = rest[:length]
			rest = rest[length:]
		default:
			body = rest
			rest = nil
		}
		newBody := n.replaceBody(body)
		if compare == "d27" && code != 100 {
			newBody = []byte("<D27>")
		}
		if compare == "json-shape" && code != 100 {
			newBody = jsonShape(newBody)
		}
		out.WriteString(status + "\r\n")
		for _, h := range headers {
			k, v := h[0], h[1]
			switch strings.ToLower(k) {
			case "date":
				v = "<DATE>"
			case "x-request-id":
				if ridPattern.MatchString(v) {
					v = "<RID>"
				}
			case "x-elysian-version":
				v = "<VER>"
			case "content-length":
				if compare == "d27" || compare == "head-only" || !bytes.Equal(newBody, body) {
					v = "<LEN>"
				}
			case "content-disposition":
				v = stampRe.ReplaceAllString(v, "<STAMP>")
			}
			v = sessionRe.ReplaceAllString(v, "edb_session=<SID>")
			v = string(n.replaceBody([]byte(v)))
			out.WriteString(k + ": " + v + "\r\n")
		}
		out.WriteString("\r\n")
		if bodyMarker != "" {
			out.WriteString(bodyMarker)
		}
		if compare == "head-only" {
			out.WriteString("<BODY OMITTED>")
			break
		}
		out.Write(newBody)
		if code != 100 && noBody && len(rest) > 0 && !bytes.HasPrefix(rest, []byte("HTTP/")) {
			break
		}
	}
	return out.Bytes()
}

func decodeChunked(b []byte) ([]byte, int, bool) {
	var out []byte
	pos := 0
	for {
		eol := bytes.Index(b[pos:], []byte("\r\n"))
		if eol < 0 {
			return out, len(b), false
		}
		sizeText, _, _ := strings.Cut(string(b[pos:pos+eol]), ";")
		size, err := strconv.ParseInt(strings.TrimSpace(sizeText), 16, 64)
		if err != nil {
			return out, len(b), false
		}
		pos += eol + 2
		if size == 0 {
			if bytes.HasPrefix(b[pos:], []byte("\r\n")) {
				pos += 2
			}
			return out, pos, true
		}
		if pos+int(size)+2 > len(b) {
			out = append(out, b[pos:min(len(b), pos+int(size))]...)
			return out, len(b), false
		}
		out = append(out, b[pos:pos+int(size)]...)
		pos += int(size) + 2
	}
}

func req(method, target string, headers []string, body string) string {
	var b strings.Builder
	b.WriteString(method + " " + target + " HTTP/1.1\r\nHost: h\r\n")
	for _, h := range headers {
		b.WriteString(h + "\r\n")
	}
	if body != "" || method == "POST" || method == "PUT" {
		b.WriteString("Content-Length: " + strconv.Itoa(len(body)) + "\r\n")
	}
	b.WriteString("\r\n")
	b.WriteString(body)
	return b.String()
}

func reqClose(method, target string, headers []string, body string) string {
	return req(method, target, append([]string{"Connection: close"}, headers...), body)
}

func simple(slug, method, target string, headers []string, body string) tcase {
	return tcase{slug: slug, steps: []step{{data: reqClose(method, target, headers, body)}}, read: readSpec{Until: "close", MaxMs: 10000}, compare: "exact"}
}

func shape(c tcase) tcase {
	c.compare = "json-shape"
	return c
}

func withCapture(c tcase, name, pattern string) tcase {
	if c.capture == nil {
		c.capture = map[string]string{}
	}
	c.capture[name] = pattern
	return c
}

func rawCase(slug string, data string, rs readSpec, compare string) tcase {
	return tcase{slug: slug, steps: []step{{data: data}}, read: rs, compare: compare}
}

var closeRead = readSpec{Until: "close", MaxMs: 10000}

func quietRead(ms int) readSpec {
	return readSpec{Until: "quiet", QuietMs: ms, MaxMs: 15000}
}

func basicAuth(user, pass string) string {
	return "Authorization: Basic " + base64.StdEncoding.EncodeToString([]byte(user+":"+pass))
}

func postJSON(addr, path, body string, headers ...string) []byte {
	conn, err := net.Dial("tcp4", addr)
	if err != nil {
		panic(err)
	}
	defer conn.Close()
	_, _ = conn.Write([]byte(reqClose("POST", path, append([]string{"Content-Type: application/json"}, headers...), body)))
	raw, _ := readResponse(conn, closeRead)
	return raw
}

func jsonBody(v any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func serverOwnError(raw []byte) bool {
	rest := raw
	for len(rest) > 0 {
		end := bytes.Index(rest, []byte("\r\n\r\n"))
		if end < 0 || !bytes.HasPrefix(rest, []byte("HTTP/")) {
			return false
		}
		head := string(rest[:end])
		lines := strings.Split(head, "\r\n")
		code := 0
		if parts := strings.SplitN(lines[0], " ", 3); len(parts) >= 2 {
			code, _ = strconv.Atoi(parts[1])
		}
		hasRID := false
		length := 0
		for _, l := range lines[1:] {
			k, v, _ := strings.Cut(l, ":")
			switch strings.ToLower(strings.TrimSpace(k)) {
			case "x-request-id":
				hasRID = true
			case "content-length":
				length, _ = strconv.Atoi(strings.TrimSpace(v))
			}
		}
		if code >= 400 && !hasRID {
			return true
		}
		rest = rest[end+4:]
		if length <= len(rest) {
			rest = rest[length:]
		} else {
			rest = nil
		}
	}
	return false
}

func shapeOf(v any) any {
	switch x := v.(type) {
	case nil:
		return "<null>"
	case bool:
		return "<bool>"
	case float64:
		return "<number>"
	case string:
		return "<string>"
	case []any:
		if len(x) == 0 {
			return []any{}
		}
		return []any{shapeOf(x[0])}
	case map[string]any:
		keys := make([]string, 0, len(x))
		for k := range x {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		out := map[string]any{}
		for _, k := range keys {
			out[k] = shapeOf(x[k])
		}
		return out
	}
	return "<unknown>"
}

func jsonShape(body []byte) []byte {
	var v any
	if err := json.Unmarshal(body, &v); err != nil {
		return []byte("<NOT JSON>")
	}
	out, err := json.Marshal(shapeOf(v))
	if err != nil {
		return []byte("<NOT JSON>")
	}
	return out
}
