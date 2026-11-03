package boot

import (
	"context"
	"fmt"
	"net"
	"sync/atomic"
	"time"

	"github.com/fasthttp/router"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

var (
	httpLog    = log.For("http")
	httpServer atomic.Pointer[fasthttp.Server]
)

func NewRouter() *router.Router {
	r := router.New()
	r.SaveMatchedRoutePath = true
	r.PanicHandler = observe.Panic
	routing.RegisterRoutes(r)

	return r
}

func NewHTTPHandler(r *router.Router) fasthttp.RequestHandler {
	return observe.Middleware(security.Protect(r.Handler))
}

func StartHTTP() {
	cfg := globals.GetConfig()

	addr := fmt.Sprintf("%s:%d", cfg.Server.HTTP.Host, cfg.Server.HTTP.Port)

	srv := &fasthttp.Server{
		Handler:               NewHTTPHandler(NewRouter()),
		Name:                  "ElysianDB",
		NoDefaultServerHeader: true,
		ReduceMemoryUsage:     true,

		Concurrency: 100_000,

		ReadBufferSize:  64 << 10,
		WriteBufferSize: 64 << 10,

		ReadTimeout:  secondsOr(cfg.Server.HTTP.ReadTimeoutSeconds, defaultHTTPTimeout),
		WriteTimeout: secondsOr(cfg.Server.HTTP.WriteTimeoutSeconds, defaultHTTPTimeout),
		IdleTimeout:  30 * time.Second,
	}

	ln, err := net.Listen("tcp4", addr)
	if err != nil {
		log.Fatal("cannot start the HTTP server", "component", "http", "addr", addr, "err", err)
	}

	httpServer.Store(srv)
	httpLog.Info("http listening", "addr", addr)

	if err := srv.Serve(ln); err != nil && !stopping.Load() {
		log.Fatal("http server stopped", "component", "http", "addr", addr, "err", err)
	}
}

const defaultHTTPTimeout = 3 * time.Second

var stopping atomic.Bool

func secondsOr(seconds int, fallback time.Duration) time.Duration {
	if seconds > 0 {
		return time.Duration(seconds) * time.Second
	}

	return fallback
}

func StopHTTP(timeout time.Duration) {
	srv := httpServer.Load()
	if srv == nil {
		return
	}

	stopping.Store(true)
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()

	if err := srv.ShutdownWithContext(ctx); err != nil {
		httpLog.Warn("http requests still running at shutdown", "err", err)
	}
}

func HTTPConnections() int {
	if srv := httpServer.Load(); srv != nil {
		return int(srv.GetOpenConnectionsCount())
	}

	return 0
}
