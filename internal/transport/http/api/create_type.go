package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/valyala/fasthttp"
)

func CreateTypeController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	err := engine.CreateEntityType(entity)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(err.Error())

		return
	}

	var payload map[string]interface{}
	if err := json.Unmarshal(ctx.PostBody(), &payload); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid json"}`)
		engine.DeleteEntityType(entity)

		return
	}

	fieldsRaw, ok := payload["fields"].(map[string]interface{})
	if !ok {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"schema.fields must be an object"}`)
		engine.DeleteEntityType(entity)

		return
	}

	storable := engine.UpdateEntitySchema(entity, fieldsRaw)

	acl.InitACL()

	out, _ := json.Marshal(storable)
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(out)
}
