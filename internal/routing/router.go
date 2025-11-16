package routing

import (
	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/transport/http/api"
	"github.com/taymour/elysiandb/internal/transport/http/controller"
	"github.com/valyala/fasthttp"
)

func RegisterRoutes(r *router.Router) {
	r.GET("/health", Version(controller.HealthController))

	r.GET("/kv/mget", Version(controller.MultiGetController))
	r.GET("/kv/{key}", Version(controller.GetKeyController))
	r.PUT("/kv/{key}", Version(controller.PutKeyController))
	r.DELETE("/kv/{key}", Version(controller.DeleteKeyController))

	r.POST("/save", Version(controller.SaveController))

	r.POST("/reset", Version(controller.ResetController))

	if globals.GetConfig().Stats.Enabled {
		r.GET("/stats", Version(controller.StatsController))
	}

	r.GET("/api/export", Version(api.ExportController))
	r.POST("/api/import", Version(api.ImportController))
	r.GET("/api/{entity}", Version(api.ListController))
	r.POST("/api/{entity}", Version(api.CreateController))
	r.GET("/api/{entity}/{id}", Version(api.GetByIdController))
	r.PUT("/api/{entity}/{id}", Version(api.UpdateByIdController))
	r.PUT("/api/{entity}", Version(api.UpdateListController))
	r.DELETE("/api/{entity}/{id}", Version(api.DeleteByIdController))
	r.DELETE("/api/{entity}", Version(api.DestroyController))
	r.POST("/api/{entity}/migrate", Version(api.MigrateController))
}

func Version(requestHandler fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		requestHandler(ctx)
		ctx.Response.Header.Add("X-Elysian-Version", globals.VERSION)
	}
}
