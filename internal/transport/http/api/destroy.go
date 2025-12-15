package api

import (
	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
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

	api_storage.DeleteAllEntities(entity)
	acl.DeleteACLForEntityType(entity)

	ctx.SetStatusCode(fasthttp.StatusNoContent)

	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Purge(entity)
	}
}
