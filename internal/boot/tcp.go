package boot

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"net"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	tcprouting "github.com/taymour/elysiandb/internal/transport/tcp/tcp_routing"
)

func InitTCP() {
	cfg := globals.GetConfig()
	addr := fmt.Sprintf("%s:%d", cfg.Server.TCP.Host, cfg.Server.TCP.Port)

	tcpAddr, err := net.ResolveTCPAddr("tcp", addr)
	if err != nil {
		log.Fatal("Error resolving TCP address:", err)
		return
	}

	ln, err := net.ListenTCP("tcp", tcpAddr)
	if err != nil {
		log.Fatal("Error starting TCP server:", err)
		return
	}
	defer ln.Close()

	log.DirectInfo("TCP server listening on ", addr)

	for {
		tc, err := ln.AcceptTCP()
		if err != nil {
			log.Error("Error accepting connection:", err)
			continue
		}

		_ = tc.SetNoDelay(true)
		_ = tc.SetKeepAlive(true)
		_ = tc.SetKeepAlivePeriod(2 * time.Minute)
		_ = tc.SetReadBuffer(256 << 10)
		_ = tc.SetWriteBuffer(256 << 10)

		go handleConnection(tc)
	}
}

const unauthenticatedReadTimeout = 30 * time.Second

func handleConnection(c net.Conn) {
	defer c.Close()
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
			var netErr net.Error
			if err != io.EOF && !(errors.As(err, &netErr) && netErr.Timeout()) {
				log.Error("read:", err)
			}
			return
		}

		resp := tcprouting.RouteLine(line, c, session)

		if len(resp) > 0 {
			if _, err := w.Write(resp); err != nil {
				log.Error("write:", err)
				return
			}
		}
		if err := w.WriteByte('\n'); err != nil {
			log.Error("write nl:", err)
			return
		}
		if err := w.Flush(); err != nil {
			log.Error("flush:", err)
			return
		}
	}
}
