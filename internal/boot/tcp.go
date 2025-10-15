package boot

import (
	"bufio"
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

	log.DirectInfo("TCP server listening on", addr)

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

func handleConnection(c net.Conn) {
	defer c.Close()
	_ = c.SetDeadline(time.Time{})

	r := bufio.NewReaderSize(c, 128<<10)
	w := bufio.NewWriterSize(c, 128<<10)

	for {
		line, err := r.ReadSlice('\n')
		if err != nil {
			if err != io.EOF {
				log.Error("read:", err)
			}
			return
		}

		resp := tcprouting.RouteLine(line, c)

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
