package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func CreateTypeController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	err := api_storage.CreateEntityType(entity)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(err.Error())

		return
	}

	var payload map[string]interface{}
	if err := json.Unmarshal(ctx.PostBody(), &payload); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid json"}`)
		api_storage.DeleteEntityType(entity)

		return
	}

	fieldsRaw, ok := payload["fields"].(map[string]interface{})
	if !ok {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"schema.fields must be an object"}`)
		api_storage.DeleteEntityType(entity)

		return
	}

	storable := api_storage.UpdateEntitySchema(entity, fieldsRaw)

	acl.InitACL()

	out, _ := json.Marshal(storable)
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(out)
}
