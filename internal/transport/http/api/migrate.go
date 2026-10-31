package api

import (
	"fmt"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/valyala/fasthttp"
)

func MigrateController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

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

	for _, query := range migrationQueries {
		if err := acl.CheckSubEntityWrites(query.Properties); err != nil {
			sendWriteError(ctx, err)
			return
		}
	}

	if err := engine.ExecuteMigrations(migrationQueries); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(fmt.Sprintf(`{"error" : "Migration failed: %s"}`, err.Error()))
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBodyString(fmt.Sprintf(`{"message" : "Entity '%s' migrated successfully."}`, entity))
}
