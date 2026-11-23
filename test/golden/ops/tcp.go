package main

import (
	"bufio"
	"bytes"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/cookiejar"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

type tcpStep struct {
	send     string
	wait     time.Duration
	quietFor time.Duration
	close    bool
}

type tcpCase struct {
	name    string
	network string
	steps   []tcpStep
}

type lockedBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (b *lockedBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *lockedBuffer) take() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	s := b.buf.String()
	b.buf.Reset()
	return s
}

var (
	tcpAddrRe = regexp.MustCompile(`(\[[^\]]+\]|\d+\.\d+\.\d+\.\d+):\d+`)
	tcpDurRe  = regexp.MustCompile(`(duration_us|duration)=[0-9.a-z\x{00b5}]+`)
	tcpErrRe  = regexp.MustCompile(`err="[^"]*"|err=[^ \n]+`)
)

func tcpErrClasses() (map[string]string, error) {
	classes := map[string]string{io.EOF.Error(): "eof", bufio.ErrBufferFull.Error(): "buffer-full"}
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return nil, err
	}
	defer ln.Close()
	accepted := make(chan net.Conn, 1)
	go func() {
		c, _ := ln.Accept()
		accepted <- c
	}()
	client, err := net.Dial("tcp", ln.Addr().String())
	if err != nil {
		return nil, err
	}
	server := <-accepted
	defer client.Close()
	_ = server.SetReadDeadline(time.Now().Add(-time.Second))
	if _, err := server.Read(make([]byte, 1)); err != nil {
		classes[tcpAddrRe.ReplaceAllString(err.Error(), "<ADDR>")] = "timeout"
	}
	server.Close()
	if _, err := server.Read(make([]byte, 1)); err != nil {
		classes[tcpAddrRe.ReplaceAllString(err.Error(), "<ADDR>")] = "closed"
	}
	if _, err := server.Write([]byte("x")); err != nil {
		classes[tcpAddrRe.ReplaceAllString(err.Error(), "<ADDR>")] = "closed"
	}
	return classes, nil
}

func maskTCPLog(s string, classes map[string]string) ([]string, []any, error) {
	var out []string
	var errs []any
	for _, line := range strings.Split(strings.TrimSpace(s), "\n") {
		if line == "" || !(strings.Contains(line, "component=tcp") || strings.Contains(line, "component=audit") || strings.Contains(line, "component=security")) {
			continue
		}
		line = cliTimeRe.ReplaceAllString(line, "<T>")
		line = cliDurRe.ReplaceAllString(line, "$1=<N>")
		var class any
		if m := tcpErrRe.FindString(line); m != "" {
			text := strings.Trim(strings.TrimPrefix(m, "err="), `"`)
			c, ok := classes[tcpAddrRe.ReplaceAllString(text, "<ADDR>")]
			if !ok {
				return nil, nil, fmt.Errorf("unclassified tcp error %q", text)
			}
			class = c
			line = strings.Replace(line, m, "err=\u27e8go:tcp-io\u27e9", 1)
		}
		line = tcpAddrRe.ReplaceAllString(line, "<ADDR>")
		out = append(out, line)
		errs = append(errs, class)
	}
	return out, errs, nil
}

func tcpConfig(mode string, httpPort, tcpPort int) string {
	host := "127.0.0.1"
	if mode == "dual" {
		host = "0.0.0.0"
		mode = "token"
	}
	auth := "    enabled: false\n"
	switch mode {
	case "token":
		auth = "    enabled: true\n    mode: token\n    token: tcp-token\n"
	case "user":
		auth = "    enabled: true\n    mode: user\n"
	case "basic":
		auth = "    enabled: true\n    mode: basic\n"
	}
	return fmt.Sprintf("store:\n  folder: data\n  shards: 4\nserver:\n  http: { enabled: true, host: 127.0.0.1, port: %d }\n  tcp: { enabled: true, host: %s, port: %d }\nlog:\n  level: debug\n  format: text\nsecurity:\n  authentication:\n%s", httpPort, host, tcpPort, auth)
}

func commonTCPCases() []tcpCase {
	long := func(n int) string { return "SET long " + strings.Repeat("x", n-len("SET long ")-2) + "\r\n" }
	return []tcpCase{
		{name: "ping", steps: []tcpStep{{send: "PING\r\n"}, {send: "ping\n"}, {send: "PiNg\r\n"}}},
		{name: "set get", steps: []tcpStep{{send: "SET foo bar\r\n"}, {send: "GET foo\r\n"}, {send: "GET missing\r\n"}, {send: "SET foo with spaces inside\r\n"}, {send: "GET foo\r\n"}}},
		{name: "ttl forms", steps: []tcpStep{{send: "SET TTL=3600 t1 v\r\n"}, {send: "SET t2 v TTL=3600\r\n"}, {send: "SET TTL=abc t3 v\r\n"}, {send: "SET TTL=-1 t4 v\r\n"}, {send: "SET TTL= t5 v\r\n"}, {send: "GET t1\r\n"}, {send: "GET t3\r\n"}}},
		{name: "mget spacing", steps: []tcpStep{{send: "SET a 1\r\n"}, {send: "SET b 2\r\n"}, {send: "MGET a b\r\n"}, {send: "MGET  a   b  missing\r\n"}, {send: "MGET\r\n"}}},
		{name: "wildcards", steps: []tcpStep{{send: "SET user:1 x\r\n"}, {send: "SET user:2 y\r\n"}, {send: "GET user:*\r\n"}, {send: "MGET user:*\r\n"}, {send: "DEL user:*\r\n"}, {send: "GET user:1\r\n"}}},
		{name: "delete", steps: []tcpStep{{send: "SET d 1\r\n"}, {send: "DEL d\r\n"}, {send: "DEL d\r\n"}, {send: "GET d\r\n"}}},
		{name: "blank lines", steps: []tcpStep{{send: "\r\n"}, {send: "   \r\n"}, {send: "\n"}, {send: "GET\tfoo\r\n"}, {send: "\tPING\r\n"}}},
		{name: "unknown", steps: []tcpStep{{send: "FOO bar\r\n"}, {send: "GET\r\n"}, {send: "SET onlykey\r\n"}, {send: "SET\r\n"}, {send: "DEL\r\n"}}},
		{name: "save reset", steps: []tcpStep{{send: "SET r 1\r\n"}, {send: "SAVE\r\n"}, {send: "RESET\r\n"}, {send: "GET r\r\n"}}},
		{name: "line 131071", steps: []tcpStep{{send: long(131071)}, {send: "GET long\r\n", quietFor: 400 * time.Millisecond}}},
		{name: "line 131072", steps: []tcpStep{{send: long(131072)}, {send: "GET long\r\n", quietFor: 400 * time.Millisecond}}},
		{name: "line 131073", steps: []tcpStep{{send: long(131073)}, {send: "PING\r\n"}}},
		{name: "line 200000", steps: []tcpStep{{send: long(200000)}, {send: "PING\r\n"}}},
		{name: "partial last line", steps: []tcpStep{{send: "SET p 1\r\n"}, {send: "GET p", close: true}}},
		{name: "exit", steps: []tcpStep{{send: "EXIT\r\n"}, {send: "PING\r\n"}}},
		{name: "ping args", steps: []tcpStep{{send: "PING x\r\n"}, {send: "PING  x y\r\n"}}},
		{name: "ttl expiry", steps: []tcpStep{{send: "SET TTL=1 e v\r\n"}, {send: "SET TTL=1 keep v\r\n"}, {send: "SET keep w\r\n", quietFor: 1600 * time.Millisecond}, {send: "GET e\r\n"}, {send: "GET keep\r\n"}, {send: "MGET e keep\r\n"}}},
		{name: "ttl parsing", steps: []tcpStep{{send: "SET ttl=3600 lc v\r\n"}, {send: "SET TTL=500x px v\r\n"}, {send: "SET TTL=99999999999999999999 ov v\r\n"}, {send: "SET TTL=+5 pl v\r\n"}, {send: "GET lc\r\n"}, {send: "GET px\r\n"}, {send: "GET TTL=99999999999999999999\r\n"}, {send: "GET TTL=-1\r\n"}, {send: "GET TTL=\r\n"}, {send: "GET TTL=+5\r\n"}}},
		{name: "set values", steps: []tcpStep{{send: "SET sp v  \r\n"}, {send: "GET sp\r\n"}, {send: "MGET sp\r\n"}, {send: "GET onlykey\r\n"}, {send: "GET \r\n"}, {send: "SET\tk\tv\r\n"}, {send: "GET k\tv\r\n"}, {send: "GET k\r\n"}, {send: "MGET a  b\r\n"}}},
		{name: "internal keys", steps: []tcpStep{{send: "SET api:entity:x v\r\n"}, {send: "GET api:entity:x\r\n"}, {send: "GET api:entity:types:list\r\n"}}},
		{name: "wildcard misses", steps: []tcpStep{{send: "GET nomatch:*\r\n"}, {send: "GET a?\r\n"}, {send: "MGET nomatch:* a\r\n"}, {send: "DEL nomatch:*\r\n"}}},
		{name: "wildcard order", steps: []tcpStep{{send: "RESET\r\n"}, {send: "SET user v\r\n"}, {send: "SET user:1 x\r\n"}, {send: "SET user:10 y\r\n"}, {send: "SET usera z\r\n"}, {send: "SET use w\r\n"}, {send: "SET user-2 q\r\n"}, {send: "GET user*\r\n"}, {send: "MGET user*\r\n"}, {send: "GET *\r\n"}, {send: "GET user?\r\n"}, {send: "DEL user*\r\n"}, {send: "GET *\r\n"}}},
		{name: "long unknown", steps: []tcpStep{{send: strings.Repeat("Z", 100) + " x\r\n"}, {send: "\u00e9t\u00e9 x\r\n"}}},
		{name: "trailing cr", steps: []tcpStep{{send: "PING\r\r\n"}, {send: "GET foo\r\r\n"}, {send: "SET cr v\r\r\n"}, {send: "GET cr\r\n"}}},
	}
}

func authTCPCases(mode string) []tcpCase {
	switch mode {
	case "token":
		return []tcpCase{
			{name: "before auth", steps: []tcpStep{{send: "PING\r\n"}, {send: "GET foo\r\n"}}},
			{name: "auth wrong", steps: []tcpStep{{send: "AUTH nope\r\n"}, {send: "GET foo\r\n"}}},
			{name: "auth token", steps: []tcpStep{{send: "AUTH tcp-token\r\n"}, {send: "SET k v\r\n"}, {send: "GET k\r\n"}, {send: "AUTH tcp-token\r\n"}, {send: "AUTH nope\r\n"}, {send: "GET k\r\n"}, {send: "RESET\r\n"}}},
			{name: "auth trimmed", steps: []tcpStep{{send: "AUTH   tcp-token  \r\n"}, {send: "GET k\r\n"}}},
			{name: "auth lowercase", steps: []tcpStep{{send: "auth tcp-token\r\n"}, {send: "GET k\r\n"}}},
			{name: "auth extra args", steps: []tcpStep{{send: "AUTH tcp-token extra\r\n"}, {send: "GET k\r\n"}}},
			{name: "unauthenticated timeout", steps: []tcpStep{{send: "", wait: 31 * time.Second}}},
		}
	case "basic", "user":
		password := "admin"
		if mode == "basic" {
			password = "ad min"
		}
		return []tcpCase{
			{name: "before auth", steps: []tcpStep{{send: "GET foo\r\n"}, {send: "PING\r\n"}}},
			{name: "auth wrong", steps: []tcpStep{{send: "AUTH admin nope\r\n"}, {send: "GET foo\r\n"}, {send: "AUTH nobody x\r\n"}}},
			{name: "auth one arg", steps: []tcpStep{{send: "AUTH admin\r\n"}, {send: "AUTH\r\n"}}},
			{name: "auth admin", steps: []tcpStep{{send: "AUTH admin " + password + "\r\n"}, {send: "SET k v\r\n"}, {send: "GET k\r\n"}, {send: "RESET\r\n"}, {send: "GET k\r\n"}}},
			{name: "auth trimmed", steps: []tcpStep{{send: "AUTH  admin " + password + "  \r\n"}, {send: "PING\r\n"}}},
			{name: "auth non-admin user", steps: []tcpStep{{send: "AUTH ann secret-1\r\n"}, {send: "GET k\r\n"}, {send: "AUTH ann wrong\r\n"}}},
		}
	}
	if mode == "dual" {
		return []tcpCase{
			{name: "ipv4 client", network: "tcp4", steps: []tcpStep{{send: "AUTH nope\r\n"}, {send: "AUTH tcp-token\r\n"}, {send: "PING\r\n"}}},
			{name: "ipv6 client", network: "tcp6", steps: []tcpStep{{send: "GET k\r\n"}, {send: "AUTH nope\r\n"}, {send: "AUTH tcp-token\r\n"}, {send: "SET k v\r\n"}, {send: "GET k\r\n"}}},
		}
	}
	return []tcpCase{{name: "auth when off", steps: []tcpStep{{send: "AUTH anything\r\n"}, {send: "PING\r\n"}}}}
}

func wildcardRead(send string) bool {
	line := strings.TrimRight(send, "\r\n")
	cmd, rest, _ := strings.Cut(line, " ")
	cmd = strings.ToUpper(cmd)
	if cmd != "GET" && cmd != "MGET" {
		return false
	}
	return !strings.Contains(rest, " ") && strings.ContainsAny(rest, "*?")
}

func sortWildcardAnswer(recv string) (string, error) {
	body, ok := strings.CutSuffix(recv, "\n")
	if !ok {
		return "", fmt.Errorf("unterminated wildcard answer %q", recv)
	}
	lines := strings.Split(body, "\n")
	for _, line := range lines {
		if strings.Count(line, "=") != 1 {
			return "", fmt.Errorf("ambiguous wildcard line %q", line)
		}
	}
	sort.Slice(lines, func(i, j int) bool {
		ki, _, _ := strings.Cut(lines[i], "=")
		kj, _, _ := strings.Cut(lines[j], "=")
		return ki < kj
	})
	return strings.Join(lines, "\n") + "\n", nil
}

func tcpCreateUser(httpPort int) error {
	jar, err := cookiejar.New(nil)
	if err != nil {
		return err
	}
	client := &http.Client{Jar: jar}
	base := "http://127.0.0.1:" + strconv.Itoa(httpPort)
	for _, step := range []struct{ path, body string }{
		{"/api/security/login", `{"username":"admin","password":"admin"}`},
		{"/api/security/user", `{"username":"ann","password":"secret-1","role":"user"}`},
	} {
		resp, err := client.Post(base+step.path, "application/json", strings.NewReader(step.body))
		if err != nil {
			return err
		}
		body, _ := io.ReadAll(resp.Body)
		resp.Body.Close()
		if resp.StatusCode/100 != 2 {
			return fmt.Errorf("%s: status %d: %s", step.path, resp.StatusCode, body)
		}
	}
	return nil
}

func readQuiet(conn net.Conn, quiet time.Duration, max time.Duration) (string, string) {
	var buf bytes.Buffer
	chunk := make([]byte, 64<<10)
	deadline := time.Now().Add(max)
	for time.Now().Before(deadline) {
		_ = conn.SetReadDeadline(time.Now().Add(quiet))
		n, err := conn.Read(chunk)
		buf.Write(chunk[:n])
		if err != nil {
			var ne net.Error
			if errors.As(err, &ne) && ne.Timeout() {
				return buf.String(), ""
			}
			if errors.Is(err, io.EOF) {
				return buf.String(), "eof"
			}
			return buf.String(), "reset"
		}
	}
	return buf.String(), ""
}

func genTCP(out string) error {
	base, err := os.MkdirTemp("", "elysiandb-golden-tcp-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(base)
	bin := filepath.Join(base, "elysiandb")
	build := exec.Command("go", "build", "-ldflags", "-X github.com/taymour/elysiandb/internal/globals.VERSION=0.3.0", "-o", bin, ".")
	build.Stderr = os.Stderr
	if err := build.Run(); err != nil {
		return err
	}
	classes, err := tcpErrClasses()
	if err != nil {
		return err
	}
	var l goldenio.Lines
	for _, mode := range []string{"off", "token", "basic", "user", "dual"} {
		dir := filepath.Join(base, mode)
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return err
		}
		tcpPort := freePort()
		for tcpPort == 8088 {
			tcpPort = freePort()
		}
		httpPort := freePort()
		os.WriteFile(filepath.Join(dir, "s.yaml"), []byte(tcpConfig(mode, httpPort, tcpPort)), 0o644)
		logs := &lockedBuffer{}
		server := exec.Command(bin, "-config", "s.yaml")
		server.Dir = dir
		server.Env = []string{"PATH=" + os.Getenv("PATH"), "HOME=" + dir, "TZ=UTC"}
		if mode == "basic" {
			server.Env = append(server.Env, "ELYSIANDB_ADMIN_PASSWORD=ad min")
		}
		server.Stdout = logs
		server.Stderr = logs
		if err := server.Start(); err != nil {
			return err
		}
		addr := "127.0.0.1:" + strconv.Itoa(tcpPort)
		for i := 0; i < 100; i++ {
			if c, err := net.Dial("tcp4", addr); err == nil {
				c.Close()
				break
			}
			time.Sleep(50 * time.Millisecond)
		}
		time.Sleep(300 * time.Millisecond)
		if mode == "user" {
			if err := tcpCreateUser(httpPort); err != nil {
				server.Process.Kill()
				return err
			}
		}
		logs.take()
		cases := append(commonTCPCases(), authTCPCases(mode)...)
		if mode != "off" {
			cases = authTCPCases(mode)
		}
		for _, c := range cases {
			network, target := "tcp4", addr
			if c.network == "tcp6" {
				network, target = "tcp6", "[::1]:"+strconv.Itoa(tcpPort)
			}
			conn, err := net.Dial(network, target)
			if err != nil {
				server.Process.Kill()
				return err
			}
			var steps []map[string]any
			for _, st := range c.steps {
				if st.send != "" {
					_, _ = conn.Write([]byte(st.send))
				}
				if st.close {
					if tc, ok := conn.(*net.TCPConn); ok {
						_ = tc.CloseWrite()
					}
				}
				quiet := 250 * time.Millisecond
				if st.quietFor > 0 {
					quiet = st.quietFor
				}
				max := 5 * time.Second
				if st.wait > 0 {
					quiet = st.wait
					max = st.wait + time.Second
				}
				recv, end := readQuiet(conn, quiet, max)
				step := map[string]any{"send_len": len(st.send)}
				if wildcardRead(st.send) && strings.Count(recv, "\n") > 1 {
					sorted, err := sortWildcardAnswer(recv)
					if err != nil {
						server.Process.Kill()
						return err
					}
					recv = sorted
					step["recv_order"] = "sorted_by_key"
				}
				step["recv"] = recv
				if len(st.send) <= 256 {
					step["send"] = st.send
				} else {
					step["send_prefix"] = st.send[:16]
				}
				if st.close {
					step["half_close"] = true
				}
				if end != "" {
					step[end] = true
				}
				steps = append(steps, step)
				if end != "" {
					break
				}
			}
			conn.Close()
			time.Sleep(100 * time.Millisecond)
			lines, errs, err := maskTCPLog(logs.take(), classes)
			if err != nil {
				server.Process.Kill()
				return err
			}
			row := map[string]any{"mode": mode, "name": c.name, "steps": steps, "log": lines, "log_err": errs}
			if c.network != "" {
				row["client"] = c.network
			}
			l.Add(row)
		}
		server.Process.Signal(os.Interrupt)
		done := make(chan error, 1)
		go func() { done <- server.Wait() }()
		select {
		case <-done:
		case <-time.After(10 * time.Second):
			server.Process.Kill()
			<-done
		}
	}
	return l.Write(out, "tcp/cases.jsonl")
}
