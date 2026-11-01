package boot

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"syscall"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/security"
	tcprouting "github.com/taymour/elysiandb/internal/transport/tcp/tcp_routing"
)

const (
	acceptBackoffMin = 5 * time.Millisecond
	acceptBackoffMax = time.Second
)

var (
	tcpLog         = log.For("tcp")
	acceptThrottle = log.NewThrottle(time.Minute)
	slowThrottle   = log.NewThrottle(time.Minute)
)

func InitTCP() {
	cfg := globals.GetConfig()
	addr := fmt.Sprintf("%s:%d", cfg.Server.TCP.Host, cfg.Server.TCP.Port)

	tcpAddr, err := net.ResolveTCPAddr("tcp", addr)
	if err != nil {
		log.Fatal("cannot resolve the TCP address", "component", "tcp", "addr", addr, "err", err)
		return
	}

	ln, err := net.ListenTCP("tcp", tcpAddr)
	if err != nil {
		log.Fatal("cannot start the TCP server", "component", "tcp", "addr", addr, "err", err)
		return
	}
	defer ln.Close()

	tcpLog.Info("tcp listening", "addr", addr)

	var delay time.Duration
	for {
		tc, err := ln.AcceptTCP()
		if err != nil {
			if errors.Is(err, net.ErrClosed) {
				return
			}

			delay = nextAcceptDelay(delay)
			if suppressed, ok := acceptThrottle.Allow("accept"); ok {
				tcpLog.Error("accept failed", log.WithSuppressed([]any{"err", err, "retry_in", delay}, suppressed)...)
			}
			time.Sleep(delay)
			continue
		}

		delay = 0

		_ = tc.SetNoDelay(true)
		_ = tc.SetKeepAlive(true)
		_ = tc.SetKeepAlivePeriod(2 * time.Minute)
		_ = tc.SetReadBuffer(256 << 10)
		_ = tc.SetWriteBuffer(256 << 10)

		go handleConnection(tc)
	}
}

func nextAcceptDelay(delay time.Duration) time.Duration {
	if delay == 0 {
		return acceptBackoffMin
	}

	delay *= 2
	if delay > acceptBackoffMax {
		return acceptBackoffMax
	}

	return delay
}

const unauthenticatedReadTimeout = 30 * time.Second

func handleConnection(c net.Conn) {
	start := time.Now()
	addr := connectionAddr(c)
	commands := 0
	debug := log.Enabled(slog.LevelDebug)

	metrics.TCPConnectionOpened()
	if debug {
		tcpLog.Debug("connection opened", "remote_addr", addr)
	}

	defer func() {
		_ = c.Close()
		metrics.TCPConnectionClosed()
		if debug {
			tcpLog.Debug("connection closed", "remote_addr", addr, "duration_ms", log.Since(start), "commands", commands)
		}
	}()

	_ = c.SetDeadline(time.Time{})

	r := bufio.NewReaderSize(c, 128<<10)
	w := bufio.NewWriterSize(c, 128<<10)
	session := tcprouting.NewSession()

	for {
		if session.IsAuthenticated() {
			_ = c.SetReadDeadline(time.Time{})
		} else {
			_ = c.SetReadDeadline(time.Now().Add(unauthenticatedReadTimeout))
		}

		line, err := r.ReadSlice('\n')
		if err != nil {
			connectionError("read", addr, err)
			return
		}

		commandStart := time.Now()
		resp := tcprouting.RouteLine(line, c, session)
		observeCommand(tcprouting.CommandName(line), time.Since(commandStart), commandStart)
		commands++

		if len(resp) > 0 {
			if _, err := w.Write(resp); err != nil {
				connectionError("write", addr, err)
				return
			}
		}
		if err := w.WriteByte('\n'); err != nil {
			connectionError("write", addr, err)
			return
		}
		if err := w.Flush(); err != nil {
			connectionError("flush", addr, err)
			return
		}
	}
}

func observeCommand(command string, d time.Duration, start time.Time) {
	metrics.ObserveTCP(command, d)

	cfg := globals.GetConfig()
	if cfg == nil || cfg.Log.SlowRequestMs <= 0 || d < time.Duration(cfg.Log.SlowRequestMs)*time.Millisecond {
		return
	}

	metrics.SlowRequest()
	if suppressed, ok := slowThrottle.Allow(command); ok {
		tcpLog.Warn("slow command", log.WithSuppressed([]any{"command", command, "duration_ms", log.Since(start)}, suppressed)...)
	}
}

func connectionAddr(c net.Conn) string {
	if c == nil || c.RemoteAddr() == nil {
		return "unknown"
	}

	return security.LoginSourceFromAddr(c.RemoteAddr()).String()
}

func connectionError(op, addr string, err error) {
	var netErr net.Error
	expected := errors.Is(err, io.EOF) ||
		errors.Is(err, net.ErrClosed) ||
		errors.Is(err, syscall.ECONNRESET) ||
		errors.Is(err, syscall.EPIPE) ||
		(errors.As(err, &netErr) && netErr.Timeout())

	if expected {
		if log.Enabled(slog.LevelDebug) {
			tcpLog.Debug("connection ended", "op", op, "remote_addr", addr, "err", err)
		}
		return
	}

	tcpLog.Warn("connection error", "op", op, "remote_addr", addr, "err", err)
}
