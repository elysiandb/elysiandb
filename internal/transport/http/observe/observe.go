package observe

import (
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"log/slog"
	"runtime/debug"
	"strconv"
	"strings"
	"sync/atomic"
	"time"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

const (
	RequestIDHeader = "X-Request-Id"
	errorKey        = "elysiandb.error"
	loggedKey       = "elysiandb.error_logged"
	requestIDKey    = "elysiandb.request_id"
	maxRequestIDLen = 64
	unmatchedRoute  = "unmatched"
)

var (
	httpLog              = log.For("http")
	securityLog          = log.For("security")
	unauthorizedThrottle = log.NewThrottle(time.Minute)
	forbiddenThrottle    = log.NewThrottle(time.Minute)
	slowThrottle         = log.NewThrottle(time.Minute)
	idPrefix             = newIDPrefix()
	idCounter            atomic.Uint64
)

var quietUnauthorizedRoutes = map[string]bool{
	"/api/security/login": true,
	"/api/security/me":    true,
}

func newIDPrefix() string {
	b := make([]byte, 4)
	if _, err := rand.Read(b); err != nil {
		return strconv.FormatInt(time.Now().UnixNano(), 36)
	}

	return hex.EncodeToString(b)
}

func SetError(ctx *fasthttp.RequestCtx, err error) {
	if err != nil {
		ctx.SetUserValue(errorKey, err)
	}
}

func RequestID(ctx *fasthttp.RequestCtx) string {
	id, _ := ctx.UserValue(requestIDKey).(string)

	return id
}

func Panic(ctx *fasthttp.RequestCtx, recovered any) {
	httpLog.Error("handler panic",
		"method", string(ctx.Method()),
		"route", Route(ctx),
		"request_id", RequestID(ctx),
		"panic", fmt.Sprint(recovered),
		"stack", string(debug.Stack()),
	)

	ctx.SetUserValue(loggedKey, true)
	ctx.ResetBody()
	ctx.SetStatusCode(fasthttp.StatusInternalServerError)
}

func Route(ctx *fasthttp.RequestCtx) string {
	if route, ok := ctx.UserValue(router.MatchedRoutePathParam).(string); ok && route != "" {
		return route
	}

	return unmatchedRoute
}

func Excluded(path []byte) bool {
	p := string(path)

	return p == "/health" ||
		p == "/metrics" ||
		p == "/admin" ||
		strings.HasPrefix(p, "/admin/") ||
		p == "/monitoring" ||
		strings.HasPrefix(p, "/monitoring/")
}

func Kind(method, route string) string {
	switch {
	case strings.HasPrefix(route, "/kv/"):
		switch method {
		case fasthttp.MethodGet:
			return metrics.KindKVGet
		case fasthttp.MethodPut:
			return metrics.KindKVSet
		case fasthttp.MethodDelete:
			return metrics.KindKVDelete
		}
	case route == "/api/query":
		return metrics.KindDocQuery
	case route == "/api/tx/{txId}/commit":
		return metrics.KindDocWrite
	case strings.HasPrefix(route, "/api/{entity}"):
		if method == fasthttp.MethodGet {
			return metrics.KindDocRead
		}
		return metrics.KindDocWrite
	}

	return ""
}

func Middleware(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		start := time.Now()
		requestID := requestIDFor(ctx)
		ctx.SetUserValue(requestIDKey, requestID)

		next(ctx)

		ctx.Response.Header.Set(RequestIDHeader, requestID)
		finish(ctx, start, requestID)
	}
}

func requestIDFor(ctx *fasthttp.RequestCtx) string {
	if id := string(ctx.Request.Header.Peek(RequestIDHeader)); validRequestID(id) {
		return id
	}

	return idPrefix + "-" + strconv.FormatUint(idCounter.Add(1), 36)
}

func validRequestID(id string) bool {
	if id == "" || len(id) > maxRequestIDLen {
		return false
	}

	for i := 0; i < len(id); i++ {
		c := id[i]
		if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' || c == '_' || c == '.' {
			continue
		}
		return false
	}

	return true
}

func finish(ctx *fasthttp.RequestCtx, start time.Time, requestID string) {
	d := time.Since(start)
	status := ctx.Response.StatusCode()
	method := string(ctx.Method())
	route := Route(ctx)
	excluded := Excluded(ctx.Path())
	user := security.GetCurrentUsername(ctx)
	durationMs := log.Since(start)

	if !excluded {
		metrics.ObserveHTTP(method, route, status, d, Kind(method, route))
	}

	switch {
	case status >= 500:
		if ctx.UserValue(loggedKey) == nil {
			args := []any{"method", method, "route", route, "status", status, "request_id", requestID, "duration_ms", durationMs}
			if user != "" {
				args = append(args, "user", user)
			}
			if err, ok := ctx.UserValue(errorKey).(error); ok {
				args = append(args, "err", err)
			}
			httpLog.Error("request failed", args...)
		}
	case status == fasthttp.StatusUnauthorized:
		unauthorized(ctx, method, route, requestID)
	case status == fasthttp.StatusForbidden:
		forbidden(ctx, method, route, user, requestID)
	case status >= 400:
		if log.Enabled(slog.LevelDebug) {
			httpLog.Debug("client error", "method", method, "route", route, "status", status, "request_id", requestID)
		}
	}

	cfg := globals.GetConfig()
	if cfg == nil || excluded {
		return
	}

	if slowMs := cfg.Log.SlowRequestMs; slowMs > 0 && d >= time.Duration(slowMs)*time.Millisecond {
		metrics.SlowRequest()
		if suppressed, ok := slowThrottle.Allow(method + " " + route); ok {
			args := []any{"method", method, "route", route, "status", status, "duration_ms", durationMs, "request_id", requestID}
			if entity := entityOf(ctx); entity != "" {
				args = append(args, "entity", entity)
			}
			httpLog.Warn("slow request", log.WithSuppressed(args, suppressed)...)
		}
	}

	if cfg.Log.AccessLog {
		args := []any{
			"method", method,
			"route", route,
			"status", status,
			"duration_ms", durationMs,
			"bytes", len(ctx.Response.Body()),
			"remote_addr", security.LoginSourceFromRequest(ctx).String(),
			"request_id", requestID,
		}
		if user != "" {
			args = append(args, "user", user)
		}
		httpLog.Info("request", args...)
	}
}

func credentialsPresented(ctx *fasthttp.RequestCtx) bool {
	return len(ctx.Request.Header.Peek(fasthttp.HeaderAuthorization)) > 0 ||
		len(ctx.Request.Header.Cookie(security.SessionCookieName)) > 0
}

func unauthorized(ctx *fasthttp.RequestCtx, method, route, requestID string) {
	if quietUnauthorizedRoutes[route] || !credentialsPresented(ctx) {
		if log.Enabled(slog.LevelDebug) {
			securityLog.Debug("unauthenticated request", "method", method, "route", route, "request_id", requestID)
		}
		return
	}

	addr := security.LoginSourceFromRequest(ctx).String()
	metrics.AuthFailure(addr)

	if suppressed, ok := unauthorizedThrottle.Allow(addr); ok {
		securityLog.Warn("unauthorized request", log.WithSuppressed([]any{
			"method", method,
			"route", route,
			"auth_mode", metrics.AuthMode(),
			"remote_addr", addr,
			"request_id", requestID,
		}, suppressed)...)
	}
}

func forbidden(ctx *fasthttp.RequestCtx, method, route, user, requestID string) {
	addr := security.LoginSourceFromRequest(ctx).String()
	if suppressed, ok := forbiddenThrottle.Allow(addr + " " + route); ok {
		args := []any{"method", method, "route", route, "remote_addr", addr, "request_id", requestID}
		if user != "" {
			args = append(args, "user", user)
		}
		securityLog.Warn("forbidden request", log.WithSuppressed(args, suppressed)...)
	}
}

func entityOf(ctx *fasthttp.RequestCtx) string {
	entity, _ := ctx.UserValue("entity").(string)

	return entity
}
