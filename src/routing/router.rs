use std::sync::Arc;

use crate::globals;
use crate::httpd::{Handler, RequestCtx, handler as h};
use crate::httprouter::Router;
use crate::kv;
use crate::log;
use crate::security;
use crate::stat;
use crate::transport::http::adminui;
use crate::transport::http::api;
use crate::transport::http::api::acl as http_acl;
use crate::transport::http::api::hook as http_hook;
use crate::transport::http::api::security as http_security;
use crate::transport::http::api::transactions as api_transaction;
use crate::transport::http::controller;
use crate::transport::http::monitoring as http_monitoring;

#[rustfmt::skip]
pub fn register_routes(r: &mut Router) {
    let cfg = match globals::config() {
        Some(cfg) => cfg,
        None => panic!("the configuration is not set"),
    };

    r.handle("GET", "/health", version(h(controller::health_controller)));

    r.handle("GET", "/kv/mget", version(security::authenticate_admin(h(controller::multi_get_controller))));
    r.handle("GET", "/kv/{key}", version(security::authenticate_admin(h(controller::get_key_controller))));
    r.handle("PUT", "/kv/{key}", version(security::authenticate_admin(h(controller::put_key_controller))));
    r.handle("DELETE", "/kv/{key}", version(security::authenticate_admin(h(controller::delete_key_controller))));

    r.handle("POST", "/save", version(security::authenticate_admin(h(controller::save_controller))));

    r.handle("POST", "/reset", version(security::authenticate_admin(h(controller::reset_controller))));

    if cfg.stats.enabled {
        r.handle("GET", "/stats", version(security::authenticate_admin(h(controller::stats_controller))));
    }

    r.handle("GET", "/api/export", version(security::authenticate_admin(h(api::export_controller))));
    r.handle("POST", "/api/import", version(security::authenticate_admin(h(api::import_controller))));
    r.handle("GET", "/api/backup", version(security::authenticate_admin(h(api::backup_controller))));
    r.handle("POST", "/api/restore", version(security::authenticate_admin(h(api::restore_controller))));
    r.handle("GET", "/api/{entity}", version(security::authenticate(api::reject_core_entity(h(api::list_controller)))));
    r.handle("POST", "/api/query", version(security::authenticate(h(api::query_controller))));
    r.handle("POST", "/api/{entity}", version(security::authenticate(api::reject_core_entity(h(api::create_controller)))));
    r.handle("GET", "/api/{entity}/{id}", version(security::authenticate(api::reject_core_entity(h(api::get_by_id_controller)))));
    r.handle("PUT", "/api/{entity}/{id}", version(security::authenticate(api::reject_core_entity(h(api::update_by_id_controller)))));
    r.handle("PUT", "/api/{entity}", version(security::authenticate(api::reject_core_entity(h(api::update_list_controller)))));
    r.handle("DELETE", "/api/{entity}/{id}", version(security::authenticate(api::reject_core_entity(h(api::delete_by_id_controller)))));
    r.handle("DELETE", "/api/{entity}", version(security::authenticate(api::reject_core_entity(h(api::destroy_controller)))));
    r.handle("GET", "/api/{entity}/count", version(security::authenticate(api::reject_core_entity(h(api::count_controller)))));
    r.handle("GET", "/api/{entity}/{id}/exists", version(security::authenticate(api::reject_core_entity(h(api::exists_controller)))));
    r.handle("POST", "/api/{entity}/migrate", version(security::authenticate_admin(api::reject_core_entity(h(api::migrate_controller)))));

    r.handle("GET", "/api/entity/types", version(security::authenticate(h(api::get_entity_types_controller))));
    r.handle("GET", "/api/entity/types/name", version(security::authenticate(h(api::get_entity_types_names_controller))));

    if cfg.api.schema.enabled {
        r.handle("POST", "/api/{entity}/create", version(security::authenticate_admin(api::reject_core_entity(h(api::create_type_controller)))));
        r.handle("GET", "/api/{entity}/schema", version(security::authenticate(api::reject_core_entity(h(api::get_schema_controller)))));
        r.handle("PUT", "/api/{entity}/schema", version(security::authenticate_admin(api::reject_core_entity(h(api::put_schema_controller)))));
    }

    r.handle("POST", "/api/tx/begin", version(security::authenticate(h(api_transaction::begin_transaction_controller))));
    r.handle("POST", "/api/tx/{txId}/rollback", version(security::authenticate(h(api_transaction::rollback_transaction_controller))));
    r.handle("POST", "/api/tx/{txId}/entity/{entity}", version(security::authenticate(api::reject_core_entity(h(api_transaction::write_transaction_controller)))));
    r.handle("PUT", "/api/tx/{txId}/entity/{entity}/{id}", version(security::authenticate(api::reject_core_entity(h(api_transaction::update_transaction_controller)))));
    r.handle("DELETE", "/api/tx/{txId}/entity/{entity}/{id}", version(security::authenticate(api::reject_core_entity(h(api_transaction::delete_transaction_controller)))));
    r.handle("POST", "/api/tx/{txId}/commit", version(security::authenticate(h(api_transaction::commit_transaction_controller))));

    r.handle("GET", "/config", version(security::authenticate_admin(h(controller::get_config_controller))));

    if security::authentication_is_enabled() && security::user_authentication_is_enabled() {
        r.handle("POST", "/api/security/user", version(adminui::admin_auth(h(http_security::create_user_controller))));
        r.handle("GET", "/api/security/user", version(adminui::admin_auth(h(http_security::get_users_controller))));
        r.handle("GET", "/api/security/user/{user_name}", version(adminui::admin_auth(h(http_security::get_user_by_username_controller))));
        r.handle("DELETE", "/api/security/user/{user_name}", version(adminui::admin_auth(h(http_security::delete_user_by_username_controller))));
        r.handle("PUT", "/api/security/user/{user_name}/password", version(adminui::admin_auth(h(http_security::change_user_password_controller))));
        r.handle("PUT", "/api/security/user/{user_name}/role", version(adminui::admin_auth(h(http_security::change_user_role_controller))));
        r.handle("POST", "/api/security/login", version(h(adminui::login_controller)));
        r.handle("POST", "/api/security/logout", version(adminui::admin_auth(h(adminui::logout_controller))));
        r.handle("GET", "/api/security/me", version(adminui::admin_auth(h(adminui::me_controller))));

        r.handle("GET", "/api/acl/{user_name}/{entity}", version(adminui::admin_auth(h(http_acl::get_acl_for_username_and_entity_controller))));
        r.handle("GET", "/api/acl/{user_name}", version(adminui::admin_auth(h(http_acl::get_all_acl_for_username))));
        r.handle("PUT", "/api/acl/{user_name}/{entity}", version(adminui::admin_auth(h(http_acl::update_acl_for_username_and_entity_controller))));
        r.handle("PUT", "/api/acl/{user_name}/{entity}/default", version(adminui::admin_auth(h(http_acl::set_default_acl_for_username_and_entity_controller))));
    }

    if cfg.api.hooks.enabled {
        r.handle("GET", "/api/hook/{entity}", version(adminui::admin_auth(h(http_hook::get_hooks_for_entity_controller))));
        r.handle("GET", "/api/hook/id/{id}", version(adminui::admin_auth(h(http_hook::get_hook_by_id_controller))));
        r.handle("POST", "/api/hook/{entity}", version(adminui::admin_auth(h(http_hook::create_hook_for_entity_controller))));
        r.handle("DELETE", "/api/hook/{entity}/{id}", version(adminui::admin_auth(h(http_hook::delete_hook_by_id_controller))));
        r.handle("PUT", "/api/hook/id/{id}", version(adminui::admin_auth(h(http_hook::update_hook_by_id_controller))));
    }

    r.handle("GET", "/monitoring/overview", version(adminui::admin_auth(h(http_monitoring::overview_controller))));
    r.handle("GET", "/monitoring/series", version(adminui::admin_auth(h(http_monitoring::series_controller))));
    r.handle("GET", "/monitoring/details", version(adminui::admin_auth(h(http_monitoring::details_controller))));
    r.handle("GET", "/monitoring/logs", version(adminui::admin_auth(h(http_monitoring::logs_controller))));
    r.handle("GET", "/monitoring/logs/download", version(adminui::admin_auth(h(http_monitoring::logs_download_controller))));
    r.handle("GET", "/monitoring/logs/level", version(adminui::admin_auth(h(http_monitoring::get_log_level_controller))));
    r.handle("PUT", "/monitoring/logs/level", version(adminui::admin_auth(h(http_monitoring::put_log_level_controller))));

    if cfg.monitoring.prometheus.enabled {
        r.handle("GET", "/metrics", version(http_monitoring::prometheus_auth(adminui::admin_auth, h(http_monitoring::prometheus_controller))));
    }

    if cfg.admin_ui.enabled {
        if !adminui::is_built() {
            log::warn("admin ui assets are missing from this binary", kv!["component" => "boot", "hint" => "build them with `make build-admin` before `cargo build`"]);
        }

        r.handle("GET", "/admin/{filepath:*}", h(adminui::admin_ui_handler));
    }
}

pub fn version(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        if globals::config().is_some_and(|cfg| cfg.stats.enabled) {
            stat::stats().increment_total_requests();
        }

        next(ctx);
        ctx.response
            .header
            .add("X-Elysian-Version", globals::VERSION);
    })
}
