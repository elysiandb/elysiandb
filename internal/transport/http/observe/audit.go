package observe

import (
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

var auditLog = log.For("audit")

func Audit(ctx *fasthttp.RequestCtx, msg string, args ...any) {
	base := []any{
		"actor", security.Actor(ctx),
		"source", "http",
		"remote_addr", security.LoginSourceFromRequest(ctx).String(),
	}

	if id := RequestID(ctx); id != "" {
		base = append(base, "request_id", id)
	}

	auditLog.Info(msg, append(base, args...)...)
}
