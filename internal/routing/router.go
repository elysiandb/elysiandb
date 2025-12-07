package routing

import (
	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	http_adminui "github.com/taymour/elysiandb/internal/transport/http/adminui"
	"github.com/taymour/elysiandb/internal/transport/http/api"
	api_transaction "github.com/taymour/elysiandb/internal/transport/http/api/transactions"
	"github.com/taymour/elysiandb/internal/transport/http/controller"
	"github.com/valyala/fasthttp"
)

func RegisterRoutes(r *router.Router) {
	r.GET("/health", Version(security.Authenticate(controller.HealthController)))

	r.GET("/kv/mget", Version(security.Authenticate(controller.MultiGetController)))
	r.GET("/kv/{key}", Version(security.Authenticate(controller.GetKeyController)))
	r.PUT("/kv/{key}", Version(security.Authenticate(controller.PutKeyController)))
	r.DELETE("/kv/{key}", Version(security.Authenticate(controller.DeleteKeyController)))

	r.POST("/save", Version(security.Authenticate(controller.SaveController)))

	r.POST("/reset", Version(security.Authenticate(controller.ResetController)))

	if globals.GetConfig().Stats.Enabled {
		r.GET("/stats", Version(security.Authenticate(controller.StatsController)))
	}

	r.GET("/api/export", Version(security.Authenticate(api.ExportController)))
	r.POST("/api/import", Version(security.Authenticate(api.ImportController)))
	r.GET("/api/{entity}", Version(security.Authenticate(api.ListController)))
	r.POST("/api/{entity}", Version(security.Authenticate(api.CreateController)))
	r.GET("/api/{entity}/{id}", Version(security.Authenticate(api.GetByIdController)))
	r.PUT("/api/{entity}/{id}", Version(security.Authenticate(api.UpdateByIdController)))
	r.PUT("/api/{entity}", Version(security.Authenticate(api.UpdateListController)))
	r.DELETE("/api/{entity}/{id}", Version(security.Authenticate(api.DeleteByIdController)))
	r.DELETE("/api/{entity}", Version(security.Authenticate(api.DestroyController)))
	r.GET("/api/{entity}/count", Version(security.Authenticate(api.CountController)))
	r.GET("/api/{entity}/{id}/exists", Version(security.Authenticate(api.ExistsController)))
	r.POST("/api/{entity}/migrate", Version(security.Authenticate(api.MigrateController)))

	r.GET("/api/entity/types", Version(security.Authenticate(api.GetEntityTypesController)))

	if globals.GetConfig().Api.Schema.Enabled {
		r.POST("/api/{entity}/create", Version(security.Authenticate(api.CreateTypeController)))
		r.GET("/api/{entity}/schema", Version(security.Authenticate(api.GetSchemaController)))
		r.PUT("/api/{entity}/schema", Version(security.Authenticate(api.PutSchemaController)))
	}

	r.POST("/api/tx/begin", Version(security.Authenticate(api_transaction.BeginTransactionController)))
	r.POST("/api/tx/{txId}/rollback", Version(security.Authenticate(api_transaction.RollbackTransactionController)))
	r.POST("/api/tx/{txId}/entity/{entity}", Version(security.Authenticate(api_transaction.WriteTransactionController)))
	r.PUT("/api/tx/{txId}/entity/{entity}/{id}", Version(security.Authenticate(api_transaction.UpdateTransactionController)))
	r.DELETE("/api/tx/{txId}/entity/{entity}/{id}", Version(security.Authenticate(api_transaction.DeleteTransactionController)))
	r.POST("/api/tx/{txId}/commit", Version(security.Authenticate(api_transaction.CommitTransactionController)))

	r.GET("/config", Version(security.Authenticate(controller.GetConfigController)))

	if globals.GetConfig().AdminUI.Enabled {
		r.POST("/admin/login", http_adminui.LoginController)
		r.POST("/admin/logout", http_adminui.AdminAuth(http_adminui.LogoutController))
		r.GET("/admin/me", http_adminui.AdminAuth(http_adminui.MeController))
		r.GET("/admin/{filepath:*}", http_adminui.AdminUIHandler)
	}
}

var Version func(requestHandler fasthttp.RequestHandler) fasthttp.RequestHandler = func(requestHandler fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		requestHandler(ctx)
		ctx.Response.Header.Add("X-Elysian-Version", globals.VERSION)
	}
}
