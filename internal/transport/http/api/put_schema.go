package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func PutSchemaController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)

	if !api_storage.EntityTypeExists(entity) {
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

	fields, ok := payload["fields"].(map[string]interface{})
	if !ok {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"schema.fields must be an object"}`)
		return
	}

	schemaData := map[string]interface{}{
		"id":      entity,
		"fields":  fields,
		"_manual": true,
	}

	api_storage.WriteEntity("schema", schemaData)

	out, _ := json.Marshal(schemaData)
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(out)
}
