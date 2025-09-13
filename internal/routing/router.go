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

	r.POST("/api/{entity}/{field}/index", api.CreateIndexController)
	r.DELETE("/api/{entity}/{field}/index", api.DeleteIndexController)

	r.GET("/api/{entity}", api.ListController)
	r.POST("/api/{entity}", api.CreateController)
	r.GET("/api/{entity}/{id}", api.GetByIdController)
	r.PUT("/api/{entity}/{id}", api.UpdateByIdController)
	r.DELETE("/api/{entity}/{id}", api.DeleteByIdController)
	r.DELETE("/api/{entity}", api.DestroyController)
}
