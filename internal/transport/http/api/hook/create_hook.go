package http_hook

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func CreateHookForEntityController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	entity := ctx.UserValue("entity").(string)
	var hookData hook.Hook
	err := json.Unmarshal(ctx.PostBody(), &hookData)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid request body"}`)

		return
	}

	hookData.Entity = entity

	err = hook.CreateHook(&hookData)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"internal server error"}`)

		return
	}

	response, err := json.Marshal(hookData)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"internal server error"}`)

		return
	}

	observe.Audit(ctx, "hook created", "entity", entity, "hook_id", hookData.ID, "event", hookData.Event, "bypass_acl", hookData.ByPassACL, "enabled", hookData.Enabled)
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}
