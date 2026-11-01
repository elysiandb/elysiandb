package api

import (
	"encoding/json"
	"fmt"
	"time"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func ImportController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	var dump map[string][]map[string]any
	if err := json.Unmarshal(ctx.PostBody(), &dump); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBody([]byte(`{"error":"invalid JSON dump"}`))
		return
	}

	ignored := removeAccountEntities(dump)
	removeInternalEntities(dump)

	for _, items := range dump {
		for _, item := range items {
			if RejectCoreEntityReferences(ctx, item) {
				return
			}
		}
	}

	if rejected := engine.ValidateImport(dump); len(rejected) > 0 {
		sendImportResponse(ctx, fasthttp.StatusBadRequest, map[string]any{
			"error":    "invalid documents, nothing was imported",
			"rejected": rejected,
		}, ignored)
		return
	}

	start := time.Now()
	rejected := engine.ImportAll(dump)
	for entity := range dump {
		purgeCache(entity, nil)
	}

	acl.InitACL()

	if len(rejected) > 0 {
		observe.SetError(ctx, fmt.Errorf("%d documents could not be imported", len(rejected)))
		observe.Audit(ctx, "import done", "entity_types", len(dump), "entities", countDocuments(dump), "rejected", len(rejected), "duration_ms", log.Since(start))
		sendImportResponse(ctx, fasthttp.StatusInternalServerError, map[string]any{
			"error":    "some documents could not be imported",
			"rejected": rejected,
		}, ignored)
		return
	}

	observe.Audit(ctx, "import done", "entity_types", len(dump), "entities", countDocuments(dump), "rejected", 0, "duration_ms", log.Since(start))
	sendImportResponse(ctx, fasthttp.StatusOK, map[string]any{"status": "import completed"}, ignored)
}

func countDocuments(dump map[string][]map[string]any) int {
	total := 0
	for _, items := range dump {
		total += len(items)
	}

	return total
}

func removeAccountEntities(dump map[string][]map[string]any) []string {
	ignored := []string{}
	for _, entity := range accountEntities {
		if _, ok := dump[entity]; ok {
			delete(dump, entity)
			ignored = append(ignored, entity)
		}
	}

	return ignored
}

func removeInternalEntities(dump map[string][]map[string]any) {
	for entity := range dump {
		if engine.IsCoreEntityType(entity) && entity != hook.HookEntity {
			delete(dump, entity)
		}
	}
}

func sendImportResponse(ctx *fasthttp.RequestCtx, status int, response map[string]any, ignored []string) {
	if len(ignored) > 0 {
		response["ignored"] = ignored
		response["message"] = accountsNotImportedMessage
	}

	body, _ := json.Marshal(response)
	ctx.SetStatusCode(status)
	ctx.SetBody(body)
}
