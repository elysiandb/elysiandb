package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
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

	if rejected := engine.ValidateImport(dump); len(rejected) > 0 {
		sendImportRejections(ctx, fasthttp.StatusBadRequest, "invalid documents, nothing was imported", rejected)
		return
	}

	rejected := engine.ImportAll(dump)
	for entity := range dump {
		purgeCache(entity, nil)
	}

	acl.InitACL()

	if len(rejected) > 0 {
		sendImportRejections(ctx, fasthttp.StatusInternalServerError, "some documents could not be imported", rejected)
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody([]byte(`{"status":"import completed"}`))
}

func sendImportRejections(ctx *fasthttp.RequestCtx, status int, message string, rejected []api_storage.ImportRejection) {
	body, _ := json.Marshal(map[string]any{
		"error":    message,
		"rejected": rejected,
	})

	ctx.SetStatusCode(status)
	ctx.SetBody(body)
}
