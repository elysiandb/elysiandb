package routing

import (
	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/security"
	http_adminui "github.com/taymour/elysiandb/internal/transport/http/adminui"
	"github.com/taymour/elysiandb/internal/transport/http/api"
	http_acl "github.com/taymour/elysiandb/internal/transport/http/api/acl"
	http_hook "github.com/taymour/elysiandb/internal/transport/http/api/hook"
	http_security "github.com/taymour/elysiandb/internal/transport/http/api/security"
	api_transaction "github.com/taymour/elysiandb/internal/transport/http/api/transactions"
	"github.com/taymour/elysiandb/internal/transport/http/controller"
	"github.com/valyala/fasthttp"
)

func RegisterRoutes(r *router.Router) {
	r.GET("/health", Version(controller.HealthController))

	r.GET("/kv/mget", Version(security.AuthenticateAdmin(controller.MultiGetController)))
	r.GET("/kv/{key}", Version(security.AuthenticateAdmin(controller.GetKeyController)))
	r.PUT("/kv/{key}", Version(security.AuthenticateAdmin(controller.PutKeyController)))
	r.DELETE("/kv/{key}", Version(security.AuthenticateAdmin(controller.DeleteKeyController)))

	r.POST("/save", Version(security.AuthenticateAdmin(controller.SaveController)))

	r.POST("/reset", Version(security.AuthenticateAdmin(controller.ResetController)))

	if globals.GetConfig().Stats.Enabled {
		r.GET("/stats", Version(security.AuthenticateAdmin(controller.StatsController)))
	}

	r.GET("/api/export", Version(security.AuthenticateAdmin(api.ExportController)))
	r.POST("/api/import", Version(security.AuthenticateAdmin(api.ImportController)))
	r.GET("/api/{entity}", Version(security.Authenticate(api.RejectCoreEntity(api.ListController))))
	r.POST("/api/query", Version(security.Authenticate(api.QueryController)))
	r.POST("/api/{entity}", Version(security.Authenticate(api.RejectCoreEntity(api.CreateController))))
	r.GET("/api/{entity}/{id}", Version(security.Authenticate(api.RejectCoreEntity(api.GetByIdController))))
	r.PUT("/api/{entity}/{id}", Version(security.Authenticate(api.RejectCoreEntity(api.UpdateByIdController))))
	r.PUT("/api/{entity}", Version(security.Authenticate(api.RejectCoreEntity(api.UpdateListController))))
	r.DELETE("/api/{entity}/{id}", Version(security.Authenticate(api.RejectCoreEntity(api.DeleteByIdController))))
	r.DELETE("/api/{entity}", Version(security.Authenticate(api.RejectCoreEntity(api.DestroyController))))
	r.GET("/api/{entity}/count", Version(security.Authenticate(api.RejectCoreEntity(api.CountController))))
	r.GET("/api/{entity}/{id}/exists", Version(security.Authenticate(api.RejectCoreEntity(api.ExistsController))))
	r.POST("/api/{entity}/migrate", Version(security.AuthenticateAdmin(api.RejectCoreEntity(api.MigrateController))))

	r.GET("/api/entity/types", Version(security.Authenticate(api.GetEntityTypesController)))
	r.GET("/api/entity/types/name", Version(security.Authenticate(api.GetEntityTypesNamesController)))

	if globals.GetConfig().Api.Schema.Enabled {
		r.POST("/api/{entity}/create", Version(security.AuthenticateAdmin(api.RejectCoreEntity(api.CreateTypeController))))
		r.GET("/api/{entity}/schema", Version(security.Authenticate(api.RejectCoreEntity(api.GetSchemaController))))
		r.PUT("/api/{entity}/schema", Version(security.AuthenticateAdmin(api.RejectCoreEntity(api.PutSchemaController))))
	}

	r.POST("/api/tx/begin", Version(security.Authenticate(api_transaction.BeginTransactionController)))
	r.POST("/api/tx/{txId}/rollback", Version(security.Authenticate(api_transaction.RollbackTransactionController)))
	r.POST("/api/tx/{txId}/entity/{entity}", Version(security.Authenticate(api.RejectCoreEntity(api_transaction.WriteTransactionController))))
	r.PUT("/api/tx/{txId}/entity/{entity}/{id}", Version(security.Authenticate(api.RejectCoreEntity(api_transaction.UpdateTransactionController))))
	r.DELETE("/api/tx/{txId}/entity/{entity}/{id}", Version(security.Authenticate(api.RejectCoreEntity(api_transaction.DeleteTransactionController))))
	r.POST("/api/tx/{txId}/commit", Version(security.Authenticate(api_transaction.CommitTransactionController)))

	r.GET("/config", Version(security.AuthenticateAdmin(controller.GetConfigController)))

	if security.AuthenticationIsEnabled() && security.UserAuthenticationIsEnabled() {
		// Security - User Management
		r.POST("/api/security/user", Version(http_adminui.AdminAuth(http_security.CreateUserController)))
		r.GET("/api/security/user", Version(http_adminui.AdminAuth(http_security.GetUsersController)))
		r.GET("/api/security/user/{user_name}", Version(http_adminui.AdminAuth(http_security.GetUserByUsernameController)))
		r.DELETE("/api/security/user/{user_name}", Version(http_adminui.AdminAuth(http_security.DeleteUserByUsernameController)))
		r.PUT("/api/security/user/{user_name}/password", Version(http_adminui.AdminAuth(http_security.ChangeUserPasswordController)))
		r.PUT("/api/security/user/{user_name}/role", Version(http_adminui.AdminAuth(http_security.ChangeUserRoleController)))
		r.POST("/api/security/login", Version(http_adminui.LoginController))
		r.POST("/api/security/logout", Version(http_adminui.AdminAuth(http_adminui.LogoutController)))
		r.GET("/api/security/me", Version(http_adminui.AdminAuth(http_adminui.MeController)))

		// ACL
		r.GET("/api/acl/{user_name}/{entity}", Version(http_adminui.AdminAuth(http_acl.GetACLForUsernameAndEntityController)))
		r.GET("/api/acl/{user_name}", Version(http_adminui.AdminAuth(http_acl.GetAllACLForUsername)))
		r.PUT("/api/acl/{user_name}/{entity}", Version(http_adminui.AdminAuth(http_acl.UpdateACLForUsernameAndEntityController)))
		r.PUT("/api/acl/{user_name}/{entity}/default", Version(http_adminui.AdminAuth(http_acl.SetDefaultACLForUsernameAndEntityController)))
	}

	// Hooks
	if globals.GetConfig().Api.Hooks.Enabled {
		r.GET("/api/hook/{entity}", Version(http_adminui.AdminAuth(http_hook.GetHooksForEntityController)))
		r.GET("/api/hook/id/{id}", Version(http_adminui.AdminAuth(http_hook.GetHookByIdController)))
		r.POST("/api/hook/{entity}", Version(http_adminui.AdminAuth(http_hook.CreateHookForEntityController)))
		r.DELETE("/api/hook/{entity}/{id}", Version(http_adminui.AdminAuth(http_hook.DeleteHookByIdController)))
		r.PUT("/api/hook/id/{id}", Version(http_adminui.AdminAuth(http_hook.UpdateHookByIdController)))
	}

	// Admin UI
	if globals.GetConfig().AdminUI.Enabled {
		if !http_adminui.IsBuilt() {
			log.DirectWarn("The Admin UI is enabled but its assets are missing from this binary: build it with `make build-admin` before `go build`")
		}

		r.GET("/admin/{filepath:*}", http_adminui.AdminUIHandler)
	}
}

var Version func(requestHandler fasthttp.RequestHandler) fasthttp.RequestHandler = func(requestHandler fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		requestHandler(ctx)
		ctx.Response.Header.Add("X-Elysian-Version", globals.VERSION)
	}
}
