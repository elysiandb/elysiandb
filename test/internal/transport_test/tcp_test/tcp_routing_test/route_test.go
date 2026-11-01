package tcprouting_test

import (
	"bytes"
	"net"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	tcprouting "github.com/taymour/elysiandb/internal/transport/tcp/tcp_routing"
)

type fakeConn struct {
	closed bool
}

func (f *fakeConn) Read(b []byte) (int, error)         { return 0, nil }
func (f *fakeConn) Write(b []byte) (int, error)        { return len(b), nil }
func (f *fakeConn) Close() error                       { f.closed = true; return nil }
func (f *fakeConn) LocalAddr() net.Addr                { return nil }
func (f *fakeConn) RemoteAddr() net.Addr               { return nil }
func (f *fakeConn) SetDeadline(t time.Time) error      { return nil }
func (f *fakeConn) SetReadDeadline(t time.Time) error  { return nil }
func (f *fakeConn) SetWriteDeadline(t time.Time) error { return nil }

func TestRouteLinePING(t *testing.T) {
	c := &fakeConn{}
	out := tcprouting.RouteLine([]byte("PING"), c, &tcprouting.Session{})
	if !bytes.Equal(out, []byte("PONG")) {
		t.Fatal("bad PING")
	}
}

func TestRouteLineEXIT(t *testing.T) {
	c := &fakeConn{}
	out := tcprouting.RouteLine([]byte("EXIT"), c, &tcprouting.Session{})
	if !bytes.Equal(out, []byte("Goodbye!")) {
		t.Fatal("bad EXIT")
	}
	if !c.closed {
		t.Fatal("conn not closed")
	}
}

func TestRouteLineUnknown(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	c := &fakeConn{}
	out := tcprouting.RouteLine([]byte("WHAT"), c, tcprouting.NewSession())
	if !bytes.Equal(out, []byte("ERR")) {
		t.Fatal("bad unknown")
	}
}

func TestRouteLineRequiresAuthWhenAuthenticationIsEnabled(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "token"
	cfg.Security.Authentication.Token = "tcp-token"
	globals.SetConfig(cfg)

	c := &fakeConn{}
	session := tcprouting.NewSession()

	if session.IsAuthenticated() {
		t.Fatal("a new session must not be authenticated when credentials are required")
	}

	if out := tcprouting.RouteLine([]byte("PING\r\n"), c, session); !bytes.Equal(out, []byte("PONG")) {
		t.Fatalf("PING must stay available, got %q", out)
	}

	for _, line := range []string{"GET foo", "MGET foo", "SET foo bar", "DEL foo", "RESET", "SAVE", "WHAT"} {
		if out := tcprouting.RouteLine([]byte(line), c, session); !bytes.Equal(out, []byte("ERR auth required")) {
			t.Fatalf("%s before AUTH: got %q", line, out)
		}
	}

	if out := tcprouting.RouteLine([]byte("AUTH nope"), c, session); !bytes.Equal(out, []byte("ERR invalid credentials")) || session.IsAuthenticated() {
		t.Fatalf("a wrong token must be refused, got %q", out)
	}

	if out := tcprouting.RouteLine([]byte("auth tcp-token\r\n"), c, session); !bytes.Equal(out, []byte("OK")) || !session.IsAuthenticated() {
		t.Fatalf("the token must authenticate the session, got %q", out)
	}

	if out := tcprouting.RouteLine([]byte("WHAT"), c, session); !bytes.Equal(out, []byte("ERR")) {
		t.Fatalf("an authenticated session must reach the router, got %q", out)
	}
}

func TestNewSessionIsAuthenticatedWithoutAuthentication(t *testing.T) {
	globals.SetConfig(&configuration.Config{})

	if !tcprouting.NewSession().IsAuthenticated() {
		t.Fatal("a session must be authenticated when authentication is disabled")
	}
}

func TestCommandName(t *testing.T) {
	cases := map[string]string{
		"GET k\n":       metrics.TCPGet,
		"get k\n":       metrics.TCPGet,
		"MGET a b\n":    metrics.TCPMget,
		"SET k v\n":     metrics.TCPSet,
		"DEL k\n":       metrics.TCPDel,
		"PING\n":        metrics.TCPOther,
		"AUTH secret\n": metrics.TCPOther,
		"FOO\n":         metrics.TCPOther,
	}

	for line, want := range cases {
		if got := tcprouting.CommandName([]byte(line)); got != want {
			t.Fatalf("CommandName(%q) = %q, want %q", line, got, want)
		}
	}
}
