package api

import (
	"fmt"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func MigrateController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !engine.IsEngineInternal() {
		ctx.SetStatusCode(fasthttp.StatusNotImplemented)
		ctx.SetBodyString(`{"error" : "Only the ElysianDB engine is supported for migrations."}`)
		return
	}

	entity := ctx.UserValue("entity").(string)

	if !engine.EntityTypeExists(entity) {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBodyString(fmt.Sprintf(`{"error" : "Entity '%s' does not exist."}`, entity))

		return
	}

	migrationQueries := api_storage.ParseMigrationQuery(string(ctx.PostBody()), entity)

	if len(migrationQueries) == 0 {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error" : "No valid migration queries found in the request body."}`)
		return
	}

	start := time.Now()
	updated, err := api_storage.ExecuteMigrationsCount(migrationQueries)
	if err != nil {
		observe.Audit(ctx, "migration failed", "entity", entity, "updated", updated, "err", err)
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(fmt.Sprintf(`{"error" : "Migration failed: %s"}`, err.Error()))
		return
	}

	observe.Audit(ctx, "migration done", "entity", entity, "queries", len(migrationQueries), "updated", updated, "duration_ms", log.Since(start))
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBodyString(fmt.Sprintf(`{"message" : "Entity '%s' migrated successfully."}`, entity))
}
