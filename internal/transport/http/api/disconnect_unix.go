//go:build unix

package api

import (
	"errors"
	"net"
	"sync/atomic"
	"syscall"
	"time"
)

const disconnectProbeInterval = 10 * time.Millisecond

func disconnectProbe(conn net.Conn) func() bool {
	for conn != nil {
		inner, ok := conn.(interface{ NetConn() net.Conn })
		if !ok {
			break
		}
		conn = inner.NetConn()
	}

	sc, ok := conn.(syscall.Conn)
	if !ok {
		return nil
	}

	raw, err := sc.SyscallConn()
	if err != nil {
		return nil
	}

	var last atomic.Int64
	var gone atomic.Bool

	return func() bool {
		if gone.Load() {
			return true
		}

		now := time.Now().UnixNano()
		prev := last.Load()
		if now-prev < int64(disconnectProbeInterval) || !last.CompareAndSwap(prev, now) {
			return false
		}

		closed := false
		if err := raw.Control(func(fd uintptr) {
			var b [1]byte
			n, _, err := syscall.Recvfrom(int(fd), b[:], syscall.MSG_PEEK|syscall.MSG_DONTWAIT)
			closed = (n == 0 && err == nil) ||
				errors.Is(err, syscall.ECONNRESET) ||
				errors.Is(err, syscall.ENOTCONN) ||
				errors.Is(err, syscall.EPIPE)
		}); err != nil {
			closed = errors.Is(err, net.ErrClosed)
		}

		if closed {
			gone.Store(true)
		}

		return closed
	}
}
