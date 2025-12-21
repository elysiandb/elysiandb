package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/valyala/fasthttp"
)

func PutSchemaController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)

	if !engine.EntityTypeExists(entity) {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBodyString(`{"error":"entity not found"}`)
		return
	}

	var payload map[string]interface{}
	if err := json.Unmarshal(ctx.PostBody(), &payload); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid json"}`)
		return
	}

	fieldsRaw, ok := payload["fields"].(map[string]interface{})
	if !ok {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"schema.fields must be an object"}`)
		return
	}

	storable := engine.UpdateEntitySchema(entity, fieldsRaw)

	out, _ := json.Marshal(storable)
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(out)
}
