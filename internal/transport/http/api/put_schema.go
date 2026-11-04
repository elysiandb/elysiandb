package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"

	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func PutSchemaController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)

	if !api_storage.EntityTypeExists(entity) {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBodyString(`{"error":"entity not found"}`)
		return
	}

	var payload map[string]any
	if err := json.Unmarshal(ctx.PostBody(), &payload); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid json"}`)

		return
	}

	fieldsRaw, ok := payload["fields"].(map[string]any)
	if !ok {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"schema.fields must be an object"}`)

		return
	}

	storable := api_storage.UpdateEntitySchema(entity, fieldsRaw)

	observe.Audit(ctx, "schema changed", "entity", entity)
	out, _ := json.Marshal(storable)
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(out)
}
