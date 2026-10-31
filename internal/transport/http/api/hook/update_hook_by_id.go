package http_hook

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func UpdateHookByIdController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	id := ctx.UserValue("id").(string)

	var hookData map[string]any
	err := json.Unmarshal(ctx.PostBody(), &hookData)
	if err != nil || hookData == nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid request body"}`)

		return
	}

	hookEntity, err := hook.GetHookById(id)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBodyString(`{"error":"hook not found"}`)

		return
	}

	hookData["id"] = id
	hookEntity.FromDataMap(hookData)

	if err := hook.UpdateHook(hookEntity); err != nil {
		response, _ := json.Marshal(map[string]string{"error": err.Error()})
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBody(response)

		return
	}

	response, err := json.Marshal(hookEntity)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"internal server error"}`)

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}
