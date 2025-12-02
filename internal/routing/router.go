package routing

import (
	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/transport/http/api"
	api_transaction "github.com/taymour/elysiandb/internal/transport/http/api/transactions"
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
	r.GET("/api/{entity}/count", Version(api.CountController))
	r.GET("/api/{entity}/{id}/exists", Version(api.ExistsController))
	r.POST("/api/{entity}/migrate", Version(api.MigrateController))

	if globals.GetConfig().Api.Schema.Enabled {
		r.GET("/api/{entity}/schema", Version(api.GetSchemaController))
		r.PUT("/api/{entity}/schema", Version(api.PutSchemaController))
	}

	r.POST("/api/tx/begin", Version(api_transaction.BeginTransactionController))
	r.POST("/api/tx/{txId}/rollback", Version(api_transaction.RollbackTransactionController))
	r.POST("/api/tx/{txId}/entity/{entity}", Version(api_transaction.WriteTransactionController))
	r.PUT("/api/tx/{txId}/entity/{entity}/{id}", Version(api_transaction.UpdateTransactionController))
	r.DELETE("/api/tx/{txId}/entity/{entity}/{id}", Version(api_transaction.DeleteTransactionController))
	r.POST("/api/tx/{txId}/commit", Version(api_transaction.CommitTransactionController))
}

func Version(requestHandler fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		requestHandler(ctx)
		ctx.Response.Header.Add("X-Elysian-Version", globals.VERSION)
	}
}
