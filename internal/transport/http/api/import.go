package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
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

	if rejected := engine.ValidateImport(dump); len(rejected) > 0 {
		sendImportResponse(ctx, fasthttp.StatusBadRequest, map[string]any{
			"error":    "invalid documents, nothing was imported",
			"rejected": rejected,
		}, ignored)
		return
	}

	rejected := engine.ImportAll(dump)
	for entity := range dump {
		purgeCache(entity, nil)
	}

	acl.InitACL()

	if len(rejected) > 0 {
		sendImportResponse(ctx, fasthttp.StatusInternalServerError, map[string]any{
			"error":    "some documents could not be imported",
			"rejected": rejected,
		}, ignored)
		return
	}

	sendImportResponse(ctx, fasthttp.StatusOK, map[string]any{"status": "import completed"}, ignored)
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

func sendImportResponse(ctx *fasthttp.RequestCtx, status int, response map[string]any, ignored []string) {
	if len(ignored) > 0 {
		response["ignored"] = ignored
		response["message"] = accountsNotImportedMessage
	}

	body, _ := json.Marshal(response)
	ctx.SetStatusCode(status)
	ctx.SetBody(body)
}
