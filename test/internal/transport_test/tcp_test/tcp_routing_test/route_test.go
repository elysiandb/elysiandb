package tcprouting_test

import (
	"bytes"
	"net"
	"testing"
	"time"

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
	out := tcprouting.RouteLine([]byte("PING"), c)
	if !bytes.Equal(out, []byte("PONG")) {
		t.Fatal("bad PING")
	}
}

func TestRouteLineEXIT(t *testing.T) {
	c := &fakeConn{}
	out := tcprouting.RouteLine([]byte("EXIT"), c)
	if !bytes.Equal(out, []byte("Goodbye!")) {
		t.Fatal("bad EXIT")
	}
	if !c.closed {
		t.Fatal("conn not closed")
	}
}

func TestRouteLineUnknown(t *testing.T) {
	c := &fakeConn{}
	out := tcprouting.RouteLine([]byte("WHAT"), c)
	if !bytes.Equal(out, []byte("ERR")) {
		t.Fatal("bad unknown")
	}
}
