package boot

import (
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

		ReadTimeout:  3 * time.Second,
		WriteTimeout: 3 * time.Second,
		IdleTimeout:  30 * time.Second,
	}

	ln, err := net.Listen("tcp4", addr)
	if err != nil {
		log.Fatal("cannot start the HTTP server", "component", "http", "addr", addr, "err", err)
	}

	httpServer.Store(srv)
	httpLog.Info("http listening", "addr", addr)

	if err := srv.Serve(ln); err != nil {
		log.Fatal("http server stopped", "component", "http", "addr", addr, "err", err)
	}
}

func HTTPConnections() int {
	if srv := httpServer.Load(); srv != nil {
		return int(srv.GetOpenConnectionsCount())
	}

	return 0
}
