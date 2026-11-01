package tcp

import (
	"bufio"
	"fmt"
	"log/slog"
	"net"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	tcprouting "github.com/taymour/elysiandb/internal/transport/tcp/tcp_routing"
)

func freeTCPPort(t *testing.T) int {
	t.Helper()

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("cannot reserve a TCP port: %v", err)
	}
	defer ln.Close()

	return ln.Addr().(*net.TCPAddr).Port
}

func waitTCPUp(addr string, timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for {
		c, err := net.DialTimeout("tcp", addr, 200*time.Millisecond)
		if err == nil {
			_ = c.Close()
			return nil
		}
		if time.Now().After(deadline) {
			return err
		}
		time.Sleep(50 * time.Millisecond)
	}
}

var (
	tcpStoreFolder string
	tcpServerOnce  sync.Once
	tcpServerAddr  string
	tcpServerPort  int
)

func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "elysiandb-tcp-e2e-")
	if err != nil {
		panic(err)
	}

	tcpStoreFolder = dir
	code := m.Run()
	_ = os.RemoveAll(dir)
	os.Exit(code)
}

func tcpConfig(security configuration.SecurityConfig) *configuration.Config {
	return &configuration.Config{
		Store: configuration.StoreConfig{
			Folder: tcpStoreFolder,
			Shards: 8,
		},
		Server: configuration.ServersConfig{
			TCP: configuration.ServerConfig{Enabled: true, Host: "127.0.0.1", Port: tcpServerPort},
		},
		Security: security,
	}
}

func startTCPServer(t *testing.T, security configuration.SecurityConfig) string {
	t.Helper()

	tcpServerOnce.Do(func() {
		tcpServerPort = freeTCPPort(t)
		globals.SetConfig(tcpConfig(security))
		storage.LoadDB()
		storage.LoadJsonDB()
		boot.BootSaver()
		boot.BootExpirationHandler()

		tcpServerAddr = fmt.Sprintf("127.0.0.1:%d", tcpServerPort)
		go boot.InitTCP()
		if err := waitTCPUp(tcpServerAddr, 2*time.Second); err != nil {
			t.Fatalf("TCP server did not start on %s: %v", tcpServerAddr, err)
		}
	})

	globals.SetConfig(tcpConfig(security))

	return tcpServerAddr
}

func TestTCP_PING_SET_MGET_GET__WILDCARD__SAVE__RESET(t *testing.T) {
	tcpAddr := startTCPServer(t, configuration.SecurityConfig{})

	c, err := net.DialTimeout("tcp", tcpAddr, 2*time.Second)
	if err != nil {
		t.Fatalf("dial: %v", err)
	}
	defer c.Close()

	r := bufio.NewReader(c)

	write := func(s string) {
		_ = c.SetWriteDeadline(time.Now().Add(1 * time.Second))
		if _, err := c.Write([]byte(s + "\n")); err != nil {
			t.Fatalf("write %q: %v", s, err)
		}
	}
	readLine := func() string {
		_ = c.SetReadDeadline(time.Now().Add(2 * time.Second))
		l, err := r.ReadString('\n')
		if err != nil {
			t.Fatalf("read: %v", err)
		}
		return l[:len(l)-1]
	}
	readN := func(n int) []string {
		out := make([]string, 0, n)
		for i := 0; i < n; i++ {
			out = append(out, readLine())
		}
		return out
	}
	contains := func(list []string, s string) bool {
		for _, x := range list {
			if x == s {
				return true
			}
		}
		return false
	}

	write("PING")
	if got := readLine(); got != "PONG" {
		t.Fatalf("want PONG, got %q", got)
	}

	write("SET foo hello")
	if got := readLine(); got != "OK" {
		t.Fatalf("want OK, got %q", got)
	}
	write("SET bar bat")
	if got := readLine(); got != "OK" {
		t.Fatalf("want OK, got %q", got)
	}
	write("SET user:1 alice")
	if got := readLine(); got != "OK" {
		t.Fatalf("want OK, got %q", got)
	}
	write("SET user:2 bob")
	if got := readLine(); got != "OK" {
		t.Fatalf("want OK, got %q", got)
	}

	write("MGET foo bar baz")
	got := readN(3)
	exp := []string{"hello", "bat", "baz=not found"}
	for i, want := range exp {
		if got[i] != want {
			t.Fatalf("MGET[%d]: want %q, got %q (all=%v)", i, want, got[i], got)
		}
	}

	write("GET foo")
	if got := readLine(); got != "foo=hello" {
		t.Fatalf("want %q, got %q", "foo=hello", got)
	}

	write("GET user:*")
	lines := readN(2)
	if !contains(lines, "user:1=alice") || !contains(lines, "user:2=bob") {
		t.Fatalf("GET user:* expected both user:1=alice and user:2=bob, got %v", lines)
	}

	write("MGET foo user:* zoo")
	mixed := readN(4)
	if mixed[0] != "hello" {
		t.Fatalf("MGET mixed[0]: want %q, got %q (all=%v)", "hello", mixed[0], mixed)
	}
	if !contains(mixed[1:3], "user:1=alice") || !contains(mixed[1:3], "user:2=bob") {
		t.Fatalf("MGET mixed user block mismatch, got %v", mixed[1:3])
	}
	if mixed[3] != "zoo=not found" {
		t.Fatalf("MGET mixed last line: want %q, got %q (all=%v)", "zoo=not found", mixed[3], mixed)
	}

	write("DEL user:*")
	if got := readLine(); got != "Deleted 2" {
		t.Fatalf("after DEL user:* want %q, got %q", "Deleted 2", got)
	}

	write("GET user:*")
	if got := readLine(); got != "user:*=not found" {
		t.Fatalf("GET user:* after delete: want %q, got %q", "user:*=not found", got)
	}

	write("MGET user:* foo")
	afterDel := readN(2)
	if afterDel[0] != "user:*=not found" {
		t.Fatalf("MGET user:* foo [0]: want %q, got %q (all=%v)", "user:*=not found", afterDel[0], afterDel)
	}
	if afterDel[1] != "hello" {
		t.Fatalf("MGET user:* foo [1]: want %q, got %q (all=%v)", "hello", afterDel[1], afterDel)
	}

	write("SAVE")
	if got := readLine(); got != "OK" {
		t.Fatalf("want OK, got %q", got)
	}

	write("GET foo")
	if got := readLine(); got != "foo=hello" {
		t.Fatalf("want %q, got %q", "foo=hello", got)
	}

	write("RESET")
	if got := readLine(); got != "OK" {
		t.Fatalf("want OK, got %q", got)
	}

	write("GET foo")
	if got := readLine(); got != "foo=not found" {
		t.Fatalf("want %q, got %q", "foo=not found", got)
	}

	write("SET to_delete hello")
	if got := readLine(); got != "OK" {
		t.Fatalf("want OK, got %q", got)
	}

	write("DEL to_delete")
	if got := readLine(); got != "Deleted 1" {
		t.Fatalf("after DEL to_delete want %q, got %q", "Deleted 1", got)
	}

	write("GET to_delete")
	if got := readLine(); got != "to_delete=not found" {
		t.Fatalf("want %q, got %q", "to_delete=not found", got)
	}
}

func dialTCP(t *testing.T, addr string) func(line string) string {
	t.Helper()

	c, err := net.DialTimeout("tcp", addr, 2*time.Second)
	if err != nil {
		t.Fatalf("dial: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })

	r := bufio.NewReader(c)

	return func(line string) string {
		_ = c.SetDeadline(time.Now().Add(2 * time.Second))
		if _, err := c.Write([]byte(line + "\r\n")); err != nil {
			t.Fatalf("write %q: %v", line, err)
		}

		reply, err := r.ReadString('\n')
		if err != nil {
			t.Fatalf("read after %q: %v", line, err)
		}

		return reply[:len(reply)-1]
	}
}

func TestTCP_TokenModeRequiresAuth(t *testing.T) {
	addr := startTCPServer(t, configuration.SecurityConfig{
		Authentication: configuration.AuthenticationConfig{Enabled: true, Mode: "token", Token: "tcp-token"},
	})

	send := dialTCP(t, addr)

	if got := send("PING"); got != "PONG" {
		t.Fatalf("PING before AUTH: want PONG, got %q", got)
	}

	for _, cmd := range []string{"SET secret hello", "GET secret", "MGET secret", "DEL secret", "SAVE", "RESET"} {
		if got := send(cmd); got != "ERR auth required" {
			t.Fatalf("%s before AUTH: want %q, got %q", cmd, "ERR auth required", got)
		}
	}

	if _, err := storage.GetByKey("secret"); err == nil {
		t.Fatalf("an unauthenticated SET must not write the key")
	}

	if got := send("AUTH wrong-token"); got != "ERR invalid credentials" {
		t.Fatalf("AUTH with a wrong token: want %q, got %q", "ERR invalid credentials", got)
	}

	if got := send("GET secret"); got != "ERR auth required" {
		t.Fatalf("GET after a failed AUTH: want %q, got %q", "ERR auth required", got)
	}

	if got := send("AUTH tcp-token"); got != "OK" {
		t.Fatalf("AUTH with the token: want OK, got %q", got)
	}

	if got := send("SET secret hello"); got != "OK" {
		t.Fatalf("SET after AUTH: want OK, got %q", got)
	}

	if got := send("GET secret"); got != "secret=hello" {
		t.Fatalf("GET after AUTH: want %q, got %q", "secret=hello", got)
	}

	other := dialTCP(t, addr)
	if got := other("GET secret"); got != "ERR auth required" {
		t.Fatalf("authentication must be per connection, got %q", got)
	}
}

func TestTCP_UserModeRequiresAdminCredentials(t *testing.T) {
	addr := startTCPServer(t, configuration.SecurityConfig{
		Authentication: configuration.AuthenticationConfig{Enabled: true, Mode: "user"},
	})

	if err := security.CreateBasicUser(&security.BasicUser{Username: "ops", Password: "ops pass", Role: security.RoleAdmin}); err != nil {
		t.Fatalf("create admin: %v", err)
	}
	if err := security.CreateBasicUser(&security.BasicUser{Username: "alice", Password: "alice-pass", Role: security.RoleUser}); err != nil {
		t.Fatalf("create user: %v", err)
	}

	send := dialTCP(t, addr)

	if got := send("GET foo"); got != "ERR auth required" {
		t.Fatalf("GET before AUTH: want %q, got %q", "ERR auth required", got)
	}

	if got := send("AUTH alice alice-pass"); got != "ERR invalid credentials" {
		t.Fatalf("AUTH as a regular user: want %q, got %q", "ERR invalid credentials", got)
	}

	if got := send("AUTH ops wrong"); got != "ERR invalid credentials" {
		t.Fatalf("AUTH with a wrong password: want %q, got %q", "ERR invalid credentials", got)
	}

	if got := send("AUTH ops ops pass"); got != "OK" {
		t.Fatalf("AUTH as an admin: want OK, got %q", got)
	}

	if got := send("SET foo bar"); got != "OK" {
		t.Fatalf("SET after AUTH: want OK, got %q", got)
	}
}

func TestTCP_NoAuthWhenAuthenticationDisabled(t *testing.T) {
	addr := startTCPServer(t, configuration.SecurityConfig{})

	send := dialTCP(t, addr)

	if got := send("SET foo bar"); got != "OK" {
		t.Fatalf("SET without authentication: want OK, got %q", got)
	}

	if got := send("AUTH anything"); got != "OK" {
		t.Fatalf("AUTH without authentication: want OK, got %q", got)
	}
}

func TestTCP_LogsSecurityAuditAndDebugEvents(t *testing.T) {
	addr := startTCPServer(t, configuration.SecurityConfig{
		Authentication: configuration.AuthenticationConfig{Enabled: true, Mode: "token", Token: "tcp-token"},
	})

	if err := log.Configure(log.Options{Level: "debug", BufferLines: 1000}); err != nil {
		t.Fatal(err)
	}
	log.SetOutput(&strings.Builder{}, false)
	tcprouting.ResetThrottles()
	t.Cleanup(func() {
		log.SetOutput(nil, false)
		_ = log.Configure(log.Options{})
	})

	send := dialTCP(t, addr)
	if got := send("AUTH wrong-token"); got != "ERR invalid credentials" {
		t.Fatalf("unexpected answer %q", got)
	}
	if got := send("AUTH tcp-token"); got != "OK" {
		t.Fatalf("unexpected answer %q", got)
	}
	if got := send("FOO\x1b[31m bar"); got != "ERR" {
		t.Fatalf("unexpected answer %q", got)
	}
	if got := send("RESET"); got != "OK" {
		t.Fatalf("unexpected answer %q", got)
	}

	find := func(q log.Query) []log.Entry {
		deadline := time.Now().Add(2 * time.Second)
		for {
			if got := log.Logs(q).Entries; len(got) > 0 || time.Now().After(deadline) {
				return got
			}
			time.Sleep(10 * time.Millisecond)
		}
	}

	if got := find(log.Query{MinLevel: slog.LevelWarn, Text: "tcp auth failed"}); len(got) != 1 || got[0].Component != "security" {
		t.Fatalf("expected a tcp auth failed warning, got %+v", got)
	}

	if got := find(log.Query{Component: "audit", Text: "store reset"}); len(got) != 1 || got[0].Attrs["actor"] != "token" || got[0].Attrs["source"] != "tcp" {
		t.Fatalf("expected an audit line for RESET, got %+v", got)
	}

	unknown := find(log.Query{MinLevel: slog.LevelDebug, Text: "unknown command"})
	if len(unknown) != 1 || unknown[0].Level != "DEBUG" || unknown[0].Attrs["command"] != "FOO\x1b[31m" {
		t.Fatalf("expected one debug line for the unknown command, got %+v", unknown)
	}

	if got := find(log.Query{MinLevel: slog.LevelDebug, Text: "connection opened"}); len(got) == 0 {
		t.Fatalf("expected a connection opened debug line")
	}
}
