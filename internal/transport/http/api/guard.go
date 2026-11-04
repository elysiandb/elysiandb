package api

import (
	"errors"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

const guardKey = "elysian.guard"

const statusClientClosedRequest = 499

func requestGuard(ctx *fasthttp.RequestCtx) *api_storage.Guard {
	if g, ok := ctx.UserValue(guardKey).(*api_storage.Guard); ok {
		return g
	}

	g := &api_storage.Guard{Deadline: api_storage.DeadlineFromConfig()}
	if cfg := globals.GetConfig(); cfg == nil || cfg.Api.Query.CancelsAbandonedRequests() {
		g.Abandoned = disconnectProbe(ctx.Conn())
	}

	ctx.SetUserValue(guardKey, g)

	return g
}

func sendGuardError(ctx *fasthttp.RequestCtx, err error) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	switch {
	case errors.Is(err, api_storage.ErrBusy):
		observe.SetRejected(ctx, "server busy")
		ctx.Response.Header.Set("Retry-After", "1")
		ctx.SetStatusCode(fasthttp.StatusServiceUnavailable)
		ctx.SetBodyString(`{"error":"server busy"}`)
	case errors.Is(err, api_storage.ErrAbandoned):
		observe.SetRejected(ctx, "client gone")
		metrics.RequestAbandoned()
		ctx.SetStatusCode(statusClientClosedRequest)
		ctx.SetBodyString(`{"error":"request abandoned by the client"}`)
	default:
		sendDeadlineExceeded(ctx)
	}
}

func guardStopped(ctx *fasthttp.RequestCtx, guard *api_storage.Guard) bool {
	if err := guard.Err(); err != nil {
		sendGuardError(ctx, err)
		return true
	}

	return false
}

func listEntities(guard *api_storage.Guard, entity string, limit, offset int, sortField string, sortAscending bool, filters map[string]map[string]string, search, includes string) []map[string]any {
	if !engine.IsEngineInternal() {
		return engine.ListEntities(entity, limit, offset, sortField, sortAscending, filters, search, includes)
	}

	data, _ := api_storage.ListEntitiesGuarded(entity, limit, offset, sortField, sortAscending, filters, search, includes, guard)

	return data
}

func executeQuery(guard *api_storage.Guard, q query.Query) ([]map[string]any, error) {
	if !engine.IsEngineInternal() {
		return engine.ExecuteQuery(q)
	}

	return api_storage.ExecuteQueryGuarded(q, guard)
}
