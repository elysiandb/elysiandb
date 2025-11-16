package routing

import (
	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/transport/http/api"
	"github.com/taymour/elysiandb/internal/transport/http/controller"
)

func RegisterRoutes(r *router.Router) {
	r.GET("/health", controller.HealthController)

	r.GET("/kv/mget", controller.MultiGetController)
	r.GET("/kv/{key}", controller.GetKeyController)
	r.PUT("/kv/{key}", controller.PutKeyController)
	r.DELETE("/kv/{key}", controller.DeleteKeyController)

	r.POST("/save", controller.SaveController)

	r.POST("/reset", controller.ResetController)

	if globals.GetConfig().Stats.Enabled {
		r.GET("/stats", controller.StatsController)
	}

	r.GET("/api/export", api.ExportController)
	r.POST("/api/import", api.ImportController)
	r.GET("/api/{entity}", api.ListController)
	r.POST("/api/{entity}", api.CreateController)
	r.GET("/api/{entity}/{id}", api.GetByIdController)
	r.PUT("/api/{entity}/{id}", api.UpdateByIdController)
	r.PUT("/api/{entity}", api.UpdateListController)
	r.DELETE("/api/{entity}/{id}", api.DeleteByIdController)
	r.DELETE("/api/{entity}", api.DestroyController)
	r.POST("/api/{entity}/migrate", api.MigrateController)
}
