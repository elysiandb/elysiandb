use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::httpd::{Request, RequestCtx, STATUS_FORBIDDEN, STATUS_UNAUTHORIZED, handler};
use elysiandb::httprouter::Router;
use elysiandb::log;
use elysiandb::routing;
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};
use elysiandb::transport::http::adminui;

use crate::tempdir::TempDir;

static SERIAL: Mutex<()> = Mutex::new(());

struct Env {
    _dir: TempDir,
    cfg: Config,
    _serial: MutexGuard<'static, ()>,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

impl Env {
    fn update(&mut self, f: impl FnOnce(&mut Config)) {
        f(&mut self.cfg);
        globals::set_config(Some(Arc::new(self.cfg.clone())));
    }
}

fn init_env(stats: bool, schema: bool, admin: bool) -> Env {
    let serial = SERIAL.lock();
    let _ = storage::close();
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.security.authentication.enabled = false;
    cfg.security.authentication.mode = String::new();
    cfg.stats.enabled = stats;
    cfg.api.schema.enabled = schema;
    cfg.admin_ui.enabled = admin;
    globals::set_config(Some(Arc::new(cfg.clone())));
    storage::load_db();
    storage::load_json_db();

    Env {
        _dir: dir,
        cfg,
        _serial: serial,
    }
}

fn local_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345)
}

fn new_router() -> Router {
    let mut r = Router::new();
    routing::register_routes(&mut r);
    r
}

fn perform(r: &Router, method: &str, path: &str) -> bool {
    let mut req = Request::new();
    req.header.set_method(method);
    req.set_request_uri(path);
    let mut ctx = RequestCtx::new();
    ctx.init(&req, Some(local_addr()));
    r.handler(&mut ctx);

    !ctx.response.header.peek("X-Elysian-Version").is_empty()
}

#[test]
fn test_version_wrapper() {
    let h = routing::version(handler(|ctx: &mut RequestCtx| {
        ctx.set_body(b"ok");
    }));
    let mut req = Request::new();
    req.header.set_method("GET");
    req.set_request_uri("/x");
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    h(&mut ctx);
    if ctx.response.body_bytes() != b"ok" {
        panic!("body mismatch");
    }
    if ctx.response.header.peek("X-Elysian-Version").is_empty() {
        panic!("missing version header");
    }
}

#[test]
fn test_register_routes_no_optional() {
    let _env = init_env(false, false, false);
    let r = new_router();

    let tests = [
        ["GET", "/health"],
        ["GET", "/kv/mget"],
        ["GET", "/kv/foo"],
        ["PUT", "/kv/foo"],
        ["DELETE", "/kv/foo"],
        ["POST", "/save"],
        ["POST", "/reset"],
        ["GET", "/api/export"],
        ["POST", "/api/import"],
        ["GET", "/api/x"],
        ["POST", "/api/x"],
        ["GET", "/api/x/123"],
        ["PUT", "/api/x/123"],
        ["PUT", "/api/x"],
        ["DELETE", "/api/x/123"],
        ["DELETE", "/api/x"],
        ["GET", "/api/x/count"],
        ["GET", "/api/x/123/exists"],
        ["POST", "/api/x/migrate"],
        ["POST", "/api/tx/begin"],
        ["POST", "/api/tx/t1/rollback"],
        ["POST", "/api/tx/t1/entity/x"],
        ["PUT", "/api/tx/t1/entity/x/1"],
        ["DELETE", "/api/tx/t1/entity/x/1"],
        ["POST", "/api/tx/t1/commit"],
        ["GET", "/config"],
    ];

    for [method, path] in tests {
        if !perform(&r, method, path) {
            panic!("route not reachable: {method} {path}");
        }
    }
}

#[test]
fn test_register_routes_with_stats() {
    let _env = init_env(true, false, false);
    let r = new_router();
    if !perform(&r, "GET", "/stats") {
        panic!("missing /stats");
    }
}

#[test]
fn test_register_routes_with_schema() {
    let _env = init_env(false, true, false);
    let r = new_router();
    if !perform(&r, "POST", "/api/books/create") {
        panic!("missing create");
    }
    if !perform(&r, "GET", "/api/books/schema") {
        panic!("missing get schema");
    }
    if !perform(&r, "PUT", "/api/books/schema") {
        panic!("missing put schema");
    }
}

fn perform_as(r: &Router, session_id: &str, method: &str, path: &str) -> u16 {
    let mut req = Request::new();
    req.header.set_method(method);
    req.set_request_uri(path);
    req.header
        .set_cookie(security::SESSION_COOKIE_NAME, session_id);
    let mut ctx = RequestCtx::new();
    ctx.init(&req, Some(local_addr()));
    r.handler(&mut ctx);

    ctx.response.status_code()
}

fn ensure_user(username: &str, role: Role) {
    if !security::basic_user_exists(username.as_bytes()) {
        let _ = security::create_basic_user(&BasicUser {
            username: username.as_bytes().to_vec(),
            password: format!("{username}-password").into_bytes(),
            role,
        });
    }
}

#[test]
fn test_register_routes_admin_routes_in_user_mode() {
    let mut env = init_env(true, true, false);
    env.update(|cfg| {
        cfg.security.authentication.enabled = true;
        cfg.security.authentication.mode = "user".to_string();
    });

    let r = new_router();

    ensure_user("bob", Role::user());
    ensure_user("admin", Role::admin());
    let hour = Duration::from_secs(3600);
    let user = security::create_session(b"bob", Role::user(), hour).unwrap_or_default();
    let admin = security::create_session(b"admin", Role::admin(), hour).unwrap_or_default();

    let admin_routes = [
        ["GET", "/kv/mget?keys=foo"],
        ["GET", "/kv/foo"],
        ["PUT", "/kv/foo"],
        ["DELETE", "/kv/foo"],
        ["POST", "/save"],
        ["POST", "/reset"],
        ["GET", "/stats"],
        ["GET", "/config"],
        ["GET", "/api/export"],
        ["POST", "/api/import"],
        ["POST", "/api/books/migrate"],
        ["POST", "/api/books/create"],
        ["PUT", "/api/books/schema"],
        ["GET", "/monitoring/overview"],
        ["GET", "/monitoring/series"],
        ["GET", "/monitoring/details"],
        ["GET", "/monitoring/logs"],
        ["GET", "/monitoring/logs/download"],
        ["GET", "/monitoring/logs/level"],
        ["PUT", "/monitoring/logs/level"],
    ];

    for [method, path] in admin_routes {
        let status = perform_as(&r, &user.id, method, path);
        if status != STATUS_FORBIDDEN {
            panic!("expected 403 for a regular user on {method} {path}, got {status}");
        }
    }

    let user_routes = [
        ["GET", "/health"],
        ["GET", "/api/books"],
        ["GET", "/api/books/schema"],
    ];

    for [method, path] in user_routes {
        let status = perform_as(&r, &user.id, method, path);
        if status == STATUS_FORBIDDEN || status == STATUS_UNAUTHORIZED {
            panic!("expected access for a regular user on {method} {path}, got {status}");
        }
    }

    for [method, path] in admin_routes {
        let status = perform_as(&r, &admin.id, method, path);
        if status == STATUS_FORBIDDEN || status == STATUS_UNAUTHORIZED {
            panic!("expected access for an admin on {method} {path}, got {status}");
        }
    }
}

#[derive(Clone, Default)]
struct Output(Arc<Mutex<Vec<u8>>>);

impl Write for Output {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(p);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_register_routes_admin_ui_warns_when_assets_are_missing() {
    let _env = init_env(false, false, true);

    let output = Output::default();
    let _ = log::configure(&log::Options::default());
    log::set_output(Some(Box::new(output.clone())), false);

    let r = new_router();

    log::set_output(None, false);
    let output = String::from_utf8_lossy(&output.0.lock()).into_owned();

    let warned = output.contains("assets are missing");
    if warned == adminui::is_built() {
        panic!(
            "expected a warning only when the Admin UI assets are missing (built={}, output={output:?})",
            adminui::is_built()
        );
    }

    let routes = r.list().get("GET").cloned().unwrap_or_default();
    if !routes.iter().any(|p| p == "/admin/{filepath:*}") {
        panic!("expected the Admin UI route to be registered, got {routes:?}");
    }
}

#[test]
fn test_register_routes_metrics_only_when_prometheus_is_enabled() {
    let mut env = init_env(false, false, false);

    let r = new_router();
    let routes = r.list().get("GET").cloned().unwrap_or_default();
    if routes.iter().any(|p| p == "/metrics") {
        panic!("/metrics must not be served when Prometheus is disabled");
    }

    env.update(|cfg| cfg.monitoring.prometheus.enabled = true);
    let r = new_router();
    let routes = r.list().get("GET").cloned().unwrap_or_default();
    if !routes.iter().any(|p| p == "/metrics") {
        panic!("/metrics must be served when Prometheus is enabled, got {routes:?}");
    }
}

#[test]
fn test_register_routes_monitoring_does_not_hide_entity_types() {
    let _env = init_env(false, false, false);

    let r = new_router();

    for path in r.list().get("GET").cloned().unwrap_or_default() {
        if path.starts_with("/api/logs") || path.starts_with("/api/monitoring") {
            panic!("{path} would hide an entity type");
        }
    }
}
