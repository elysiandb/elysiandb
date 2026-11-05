package boot

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"sync/atomic"
	"time"

	"github.com/fasthttp/router"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/security"
	http_api "github.com/taymour/elysiandb/internal/transport/http/api"
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
	return observe.Middleware(refuseWritesWithoutDisk(security.Protect(r.Handler)))
}

func changesData(ctx *fasthttp.RequestCtx) bool {
	if ctx.IsGet() || ctx.IsHead() || ctx.IsOptions() {
		return false
	}

	path := ctx.Path()
	switch {
	case bytes.HasPrefix(path, []byte("/kv/")):
		return true
	case !bytes.HasPrefix(path, []byte("/api/")) || !engine.IsEngineInternal():
		return false
	case bytes.Equal(path, []byte("/api/query")), bytes.HasPrefix(path, []byte("/api/security/log")):
		return false
	}

	return true
}

func refuseWritesWithoutDisk(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		if !changesData(ctx) {
			next(ctx)
			return
		}

		if err := storage.WriteRefusal(); err != nil && !ctx.IsDelete() {
			storage.NoteRefusedWrite()
			sendRefusedWrite(ctx, err)
			return
		}

		before := storage.RefusedWrites()
		next(ctx)

		if storage.RefusedWrites() != before && !ctx.Hijacked() {
			err := storage.WriteRefusal()
			if err == nil {
				err = datastore.ErrWriteFailed
			}
			sendRefusedWrite(ctx, err)
		}
	}
}

func sendRefusedWrite(ctx *fasthttp.RequestCtx, err error) {
	status := fasthttp.StatusServiceUnavailable
	if errors.Is(err, datastore.ErrDiskFull) {
		status = fasthttp.StatusInsufficientStorage
	}

	ctx.Response.Reset()
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.Response.Header.Set("Retry-After", "30")
	ctx.SetStatusCode(status)
	body, _ := json.Marshal(map[string]string{"error": err.Error()})
	ctx.SetBody(body)
	observe.SetRejected(ctx, err.Error())
}

func NewHTTPServer() *fasthttp.Server {
	cfg := globals.GetConfig()

	return &fasthttp.Server{
		Handler:               http_api.CloseAfterUnreadBody(NewHTTPHandler(NewRouter())),
		HeaderReceived:        http_api.StreamingRoutes,
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
}

func StartHTTP() {
	cfg := globals.GetConfig()

	addr := fmt.Sprintf("%s:%d", cfg.Server.HTTP.Host, cfg.Server.HTTP.Port)

	srv := NewHTTPServer()

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
