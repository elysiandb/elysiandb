package api

import (
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func DestroyController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)

	if security.UserAuthenticationIsEnabled() && !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBody([]byte(`{"error":"only admin users can destroy entities"}`))
		return
	}

	engine.DeleteAllEntities(entity)
	acl.DeleteACLForEntityType(entity)

	ctx.SetStatusCode(fasthttp.StatusNoContent)

	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Purge(entity)
	}
}
