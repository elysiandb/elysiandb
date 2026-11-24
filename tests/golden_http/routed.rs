use std::collections::BTreeMap;
use std::io::Write;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use elysiandb::acl;
use elysiandb::configuration::{self, Config};
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::httpd::{Handler, RequestCtx, handler};
use elysiandb::httprouter::Router;
use elysiandb::log;
use elysiandb::metrics;
use elysiandb::security;
use elysiandb::stat;
use elysiandb::storage;
use elysiandb::transport::http::api::{acl as http_acl, security as http_security};
use elysiandb::transport::http::{adminui, controller, monitoring, observe};

use super::{Case, LogBuffer, Running, load_case, new_http_server_config, read_response};
use crate::common;
use crate::normalize::Normalizer;
use crate::tempdir::TempDir;

pub const OFF: [&str; 52] = [
    "001-health",
    "002-health-head",
    "003-kv-put",
    "004-kv-put-ttl",
    "005-kv-get",
    "006-kv-put-escaped",
    "007-kv-get-escaped",
    "008-kv-get-unescaped",
    "009-kv-mget",
    "010-kv-put-mget",
    "011-kv-get-missing",
    "012-kv-delete",
    "013-kv-get-deleted",
    "014-save",
    "015-stats",
    "016-config",
    "065-monitoring-logs-download",
    "066-monitoring-level-get",
    "067-monitoring-level-put",
    "068-monitoring-level-put-bad",
    "069-metrics",
    "072-not-found",
    "073-method-not-allowed",
    "074-patch",
    "075-trace",
    "076-connect",
    "077-unknown-method",
    "078-options-no-origin",
    "079-options-origin",
    "080-trailing-slash-get",
    "081-trailing-slash-post",
    "082-case-redirect",
    "083-double-slash",
    "084-dot-segment",
    "086-http10-no-host-redirect",
    "090-admin-redirect",
    "091-admin-root",
    "092-admin-gitkeep",
    "093-admin-spa",
    "094-admin-asset-missing",
    "095-admin-dotdot-panic",
    "101-body-4mib-exact",
    "103-body-4mib-chunked",
    "105-head-60kib",
    "108-http10-close",
    "109-http10-keepalive",
    "110-connection-close",
    "111-http2-token",
    "114-absolute-form",
    "116-request-id-valid",
    "117-request-id-invalid",
    "129-reset",
];

pub const BASIC: [&str; 25] = [
    "001-no-credentials",
    "002-wrong-credentials",
    "003-malformed-header",
    "004-bearer-in-basic-mode",
    "005-health",
    "006-health-head",
    "007-kv-put",
    "008-kv-put-ttl",
    "009-kv-get",
    "010-kv-put-escaped",
    "011-kv-get-escaped",
    "012-kv-get-unescaped",
    "013-kv-mget",
    "014-kv-put-mget",
    "015-kv-get-missing",
    "016-kv-delete",
    "017-kv-get-deleted",
    "018-save",
    "019-stats",
    "020-config",
    "069-monitoring-logs-download",
    "070-monitoring-level-get",
    "071-monitoring-level-put",
    "072-monitoring-level-put-bad",
    "073-metrics",
];

pub const TOKEN: [&str; 9] = [
    "001-no-credentials",
    "002-wrong-token",
    "003-lowercase-bearer",
    "006-kv",
    "007-config",
    "008-metrics-no-token",
    "009-metrics-wrong-token",
    "010-metrics-token",
    "011-metrics-api-token",
];

pub const USER: [&str; 20] = [
    "001-no-credentials",
    "002-health-open",
    "003-login-bad",
    "004-login-malformed",
    "005-login-admin",
    "006-me",
    "007-user-create",
    "008-user-create-duplicate",
    "009-users",
    "010-user-get",
    "011-user-password",
    "012-user-role",
    "013-user-role-bad",
    "020-config-as-admin",
    "021-kv-as-admin",
    "022-basic-header-in-user-mode",
    "023-user-delete",
    "024-user-delete-unknown",
    "025-logout",
    "026-me-after-logout",
];

pub const CORS: [&str; 2] = ["001-preflight-allowed", "002-preflight-denied"];

pub const DEADLINE: [&str; 1] = ["003-health"];

const ADMIN_UI_CASES: [&str; 6] = [
    "090-admin-redirect",
    "091-admin-root",
    "092-admin-gitkeep",
    "093-admin-spa",
    "094-admin-asset-missing",
    "095-admin-dotdot-panic",
];

const D17_RENAMES: [(&str, &str); 5] = [
    ("go_version", "rust_version"),
    ("goroutines", "threads"),
    ("gomaxprocs", "usable_cpus"),
    ("heap_bytes", "allocated_bytes"),
    ("heap_live_bytes", "memory_live_bytes"),
];

const D17_REMOVED: [&str; 2] = ["gc_pause_p99_ms", "gc_cycles_rps"];

const D17_PPROF: &str = ",\"Pprof\":{\"Enabled\":false}";

fn version(next: Handler) -> Handler {
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

fn stub(reached: &Arc<AtomicBool>) -> Handler {
    let flag = reached.clone();
    Arc::new(move |_ctx: &mut RequestCtx| flag.store(true, Ordering::SeqCst))
}

fn register_routes(r: &mut Router, cfg: &Config, reached: &Arc<AtomicBool>) {
    let s = || stub(reached);
    let admin = security::authenticate_admin;
    let auth = security::authenticate;
    let ui = adminui::admin_auth;

    r.handle(
        "GET",
        "/health",
        version(handler(controller::health_controller)),
    );

    r.handle(
        "GET",
        "/kv/mget",
        version(admin(handler(controller::multi_get_controller))),
    );
    r.handle(
        "GET",
        "/kv/{key}",
        version(admin(handler(controller::get_key_controller))),
    );
    r.handle(
        "PUT",
        "/kv/{key}",
        version(admin(handler(controller::put_key_controller))),
    );
    r.handle(
        "DELETE",
        "/kv/{key}",
        version(admin(handler(controller::delete_key_controller))),
    );

    r.handle(
        "POST",
        "/save",
        version(admin(handler(controller::save_controller))),
    );

    r.handle(
        "POST",
        "/reset",
        version(admin(handler(controller::reset_controller))),
    );

    if cfg.stats.enabled {
        r.handle(
            "GET",
            "/stats",
            version(admin(handler(controller::stats_controller))),
        );
    }

    r.handle("GET", "/api/export", version(admin(s())));
    r.handle("POST", "/api/import", version(admin(s())));
    r.handle("GET", "/api/backup", version(admin(s())));
    r.handle("POST", "/api/restore", version(admin(s())));
    r.handle("GET", "/api/{entity}", version(auth(s())));
    r.handle("POST", "/api/query", version(auth(s())));
    r.handle("POST", "/api/{entity}", version(auth(s())));
    r.handle("GET", "/api/{entity}/{id}", version(auth(s())));
    r.handle("PUT", "/api/{entity}/{id}", version(auth(s())));
    r.handle("PUT", "/api/{entity}", version(auth(s())));
    r.handle("DELETE", "/api/{entity}/{id}", version(auth(s())));
    r.handle("DELETE", "/api/{entity}", version(auth(s())));
    r.handle("GET", "/api/{entity}/count", version(auth(s())));
    r.handle("GET", "/api/{entity}/{id}/exists", version(auth(s())));
    r.handle("POST", "/api/{entity}/migrate", version(admin(s())));

    r.handle("GET", "/api/entity/types", version(auth(s())));
    r.handle("GET", "/api/entity/types/name", version(auth(s())));

    if cfg.api.schema.enabled {
        r.handle("POST", "/api/{entity}/create", version(admin(s())));
        r.handle("GET", "/api/{entity}/schema", version(auth(s())));
        r.handle("PUT", "/api/{entity}/schema", version(admin(s())));
    }

    r.handle("POST", "/api/tx/begin", version(auth(s())));
    r.handle("POST", "/api/tx/{txId}/rollback", version(auth(s())));
    r.handle("POST", "/api/tx/{txId}/entity/{entity}", version(auth(s())));
    r.handle(
        "PUT",
        "/api/tx/{txId}/entity/{entity}/{id}",
        version(auth(s())),
    );
    r.handle(
        "DELETE",
        "/api/tx/{txId}/entity/{entity}/{id}",
        version(auth(s())),
    );
    r.handle("POST", "/api/tx/{txId}/commit", version(auth(s())));

    r.handle(
        "GET",
        "/config",
        version(admin(handler(controller::get_config_controller))),
    );

    if security::authentication_is_enabled() && security::user_authentication_is_enabled() {
        r.handle(
            "POST",
            "/api/security/user",
            version(ui(handler(http_security::create_user_controller))),
        );
        r.handle(
            "GET",
            "/api/security/user",
            version(ui(handler(http_security::get_users_controller))),
        );
        r.handle(
            "GET",
            "/api/security/user/{user_name}",
            version(ui(handler(http_security::get_user_by_username_controller))),
        );
        r.handle(
            "DELETE",
            "/api/security/user/{user_name}",
            version(ui(handler(
                http_security::delete_user_by_username_controller,
            ))),
        );
        r.handle(
            "PUT",
            "/api/security/user/{user_name}/password",
            version(ui(handler(http_security::change_user_password_controller))),
        );
        r.handle(
            "PUT",
            "/api/security/user/{user_name}/role",
            version(ui(handler(http_security::change_user_role_controller))),
        );
        r.handle(
            "POST",
            "/api/security/login",
            version(handler(adminui::login_controller)),
        );
        r.handle(
            "POST",
            "/api/security/logout",
            version(ui(handler(adminui::logout_controller))),
        );
        r.handle(
            "GET",
            "/api/security/me",
            version(ui(handler(adminui::me_controller))),
        );

        r.handle(
            "GET",
            "/api/acl/{user_name}/{entity}",
            version(ui(handler(
                http_acl::get_acl_for_username_and_entity_controller,
            ))),
        );
        r.handle(
            "GET",
            "/api/acl/{user_name}",
            version(ui(handler(http_acl::get_all_acl_for_username))),
        );
        r.handle(
            "PUT",
            "/api/acl/{user_name}/{entity}",
            version(ui(handler(
                http_acl::update_acl_for_username_and_entity_controller,
            ))),
        );
        r.handle(
            "PUT",
            "/api/acl/{user_name}/{entity}/default",
            version(ui(handler(
                http_acl::set_default_acl_for_username_and_entity_controller,
            ))),
        );
    }

    if cfg.api.hooks.enabled {
        r.handle("GET", "/api/hook/{entity}", version(ui(s())));
        r.handle("GET", "/api/hook/id/{id}", version(ui(s())));
        r.handle("POST", "/api/hook/{entity}", version(ui(s())));
        r.handle("DELETE", "/api/hook/{entity}/{id}", version(ui(s())));
        r.handle("PUT", "/api/hook/id/{id}", version(ui(s())));
    }

    r.handle(
        "GET",
        "/monitoring/overview",
        version(ui(handler(monitoring::overview_controller))),
    );
    r.handle(
        "GET",
        "/monitoring/series",
        version(ui(handler(monitoring::series_controller))),
    );
    r.handle(
        "GET",
        "/monitoring/details",
        version(ui(handler(monitoring::details_controller))),
    );
    r.handle(
        "GET",
        "/monitoring/logs",
        version(ui(handler(monitoring::logs_controller))),
    );
    r.handle(
        "GET",
        "/monitoring/logs/download",
        version(ui(handler(monitoring::logs_download_controller))),
    );
    r.handle(
        "GET",
        "/monitoring/logs/level",
        version(ui(handler(monitoring::get_log_level_controller))),
    );
    r.handle(
        "PUT",
        "/monitoring/logs/level",
        version(ui(handler(monitoring::put_log_level_controller))),
    );

    if cfg.monitoring.prometheus.enabled {
        r.handle(
            "GET",
            "/metrics",
            version(monitoring::prometheus_auth(
                ui,
                handler(monitoring::prometheus_controller),
            )),
        );
    }

    if cfg.admin_ui.enabled {
        r.handle(
            "GET",
            "/admin/{filepath:*}",
            handler(adminui::admin_ui_handler),
        );
    }
}

struct Booted {
    data: TempDir,
    _conf: TempDir,
    logs: LogBuffer,
    reached: Arc<AtomicBool>,
    running: Running,
}

impl Drop for Booted {
    fn drop(&mut self) {
        let _ = self
            .running
            .server
            .shutdown_with_timeout(Duration::from_secs(5));
        let _ = storage::close();
        log::set_output(None, false);
        let _ = log::configure(&log::Options::default());
        metrics::reset();
    }
}

fn boot(scenario: &str) -> Booted {
    let data = TempDir::new();
    let conf = TempDir::new();
    let folder = data.path().to_string_lossy().into_owned();
    let yaml = String::from_utf8_lossy(&common::read_golden(&format!(
        "http/{scenario}/config.yaml"
    )))
    .replace("<DATA>", &folder);
    let path = conf.path().join("config.yaml");
    if let Err(e) = std::fs::write(&path, yaml) {
        panic!("{scenario}: {e}");
    }
    let cfg = match configuration::load_config(&path) {
        Ok(cfg) => cfg,
        Err(e) => panic!("{scenario}: {e}"),
    };
    globals::set_config(Some(Arc::new(cfg.clone())));

    let logs = LogBuffer::default();
    if let Err(e) = log::configure(&log::Options {
        level: cfg.log.level.clone(),
        format: cfg.log.format.clone(),
        buffer_lines: cfg.log.buffer_lines,
    }) {
        panic!("{scenario}: {e}");
    }
    log::set_output(Some(Box::new(logs.clone())), false);
    metrics::reset();
    observe::reset_throttles();
    observe::install_panic_hook();
    stat::init();
    globals::set_starting(false);
    security::reset_login_attempts();

    if let Err(e) = storage::open(storage::MODE_SERVER) {
        panic!("{scenario}: {e}");
    }
    let _ = acl::init_acl();
    let _ = security::init_admin_user_if_not_exists();
    security::protect_sessions_file();

    let reached = Arc::new(AtomicBool::new(false));
    let mut r = Router::new();
    r.save_matched_route_path = true;
    r.panic_handler = Some(observe::panic_handler());
    register_routes(&mut r, &cfg, &reached);

    let chain = observe::middleware(security::protect(r.into_handler()));
    let mut server_cfg = new_http_server_config(100_000);
    server_cfg.handler = chain;
    let running = Running::start(server_cfg);

    Booted {
        data,
        _conf: conf,
        logs,
        reached,
        running,
    }
}

fn capture(pattern: &str, raw: &[u8]) -> Option<String> {
    let open = pattern.find("([")?;
    let close = pattern[open..].find("]+)")? + open;
    let prefix = &pattern.as_bytes()[..open];
    let class = &pattern[open + 2..close];
    let suffix = &pattern.as_bytes()[close + 3..];
    let accepts = |c: u8| match class {
        "0-9a-f" => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        "^\"" => c != b'"',
        other => panic!("unsupported capture class {other}"),
    };

    let mut from = 0;
    while let Some(i) = raw[from..].windows(prefix.len()).position(|w| w == prefix) {
        let start = from + i + prefix.len();
        let len = raw[start..].iter().take_while(|&&c| accepts(c)).count();
        if len > 0 && raw[start + len..].starts_with(suffix) {
            return Some(String::from_utf8_lossy(&raw[start..start + len]).into_owned());
        }
        from = from + i + 1;
    }
    None
}

fn exchange(case: &Case, addr: SocketAddr, captured: &BTreeMap<String, String>) -> (Vec<u8>, bool) {
    let mut conn = TcpStream::connect(addr).unwrap_or_else(|e| panic!("{}: {e}", case.id));
    for (file, pause) in &case.steps {
        if *pause > 0 {
            std::thread::sleep(Duration::from_millis(*pause));
        }
        let mut data = common::read_golden(&format!("http/{}/{file}", case.scenario));
        for (name, value) in captured {
            let placeholder = format!("{{{{{name}}}}}");
            let text = String::from_utf8_lossy(&data).into_owned();
            if text.contains(&placeholder) {
                data = text.replace(&placeholder, value).into_bytes();
            }
        }
        let _ = conn.write_all(&data);
    }
    if case.half_close {
        let _ = conn.shutdown(Shutdown::Write);
    }
    read_response(&mut conn, case.until_close, case.quiet_ms, case.max_ms)
}

fn rename_keys(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                if D17_REMOVED.contains(&k.as_str()) {
                    continue;
                }
                let key = D17_RENAMES
                    .iter()
                    .find(|(from, _)| from == k)
                    .map(|(_, to)| to.to_string())
                    .unwrap_or_else(|| k.clone());
                out.insert(key, rename_keys(x));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(rename_keys).collect()),
        other => other.clone(),
    }
}

fn with_d17_names(golden: &[u8]) -> Vec<u8> {
    let Some(end) = golden.windows(4).position(|w| w == b"\r\n\r\n") else {
        return golden.to_vec();
    };
    let body = &golden[end + 4..];
    let Ok(value) = json::unmarshal_value(body) else {
        return golden.to_vec();
    };
    let Ok(renamed) = json::marshal(&rename_keys(&value)) else {
        return golden.to_vec();
    };
    let mut out = golden[..end + 4].to_vec();
    out.extend_from_slice(&renamed);
    out
}

fn compare(case: &Case, norm: &mut Normalizer, raw: &[u8], closed: bool, logs: &str) {
    let mut want = common::read_golden(&format!("http/{}/{}.resp", case.scenario, case.id));
    if case.compare == "json-shape" {
        want = with_d17_names(&want);
    }
    let text = String::from_utf8_lossy(&want).into_owned();
    if text.contains(D17_PPROF) {
        want = text.replace(D17_PPROF, "").into_bytes();
    }
    let got = norm.normalize(raw, case.head, &case.compare);
    assert_eq!(
        String::from_utf8_lossy(&got),
        String::from_utf8_lossy(&want),
        "{}/{}: normalized response differs",
        case.scenario,
        case.id
    );
    assert_eq!(
        closed, case.closed,
        "{}/{}: closed flag",
        case.scenario, case.id
    );
    let logged = logs.contains("msg=\"http connection error\"")
        || logs.contains("msg=\"http connection limit reached\"");
    assert_eq!(
        logged, case.logged,
        "{}/{}: logged flag, log output: {logs}",
        case.scenario, case.id
    );
}

fn captures_of(scenario: &str, id: &str) -> Vec<(String, String)> {
    let cases = common::read_json(&format!("http/{scenario}/cases.json"));
    let row = cases
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
        })
        .unwrap_or_else(|| panic!("{scenario}/{id}: missing case"));
    match row.get("capture").and_then(Value::as_object) {
        None => Vec::new(),
        Some(m) => m
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
            .collect(),
    }
}

pub fn replay(scenario: &str, ids: &[&str]) -> usize {
    let booted = boot(scenario);
    let folder = booted.data.path().to_string_lossy().into_owned();
    let mut norm = Normalizer::new(&folder, &std::env::temp_dir().to_string_lossy());
    let mut captured: BTreeMap<String, String> = BTreeMap::new();
    let built = adminui::is_built();
    let mut replayed = 0;

    for id in ids {
        if built && ADMIN_UI_CASES.contains(id) {
            continue;
        }
        let case = load_case(scenario, id);
        booted.reached.store(false, Ordering::SeqCst);
        let mark = booted.logs.len();
        let (raw, closed) = exchange(&case, booted.running.addr, &captured);
        std::thread::sleep(Duration::from_millis(20));
        assert!(
            !booted.reached.load(Ordering::SeqCst),
            "{scenario}/{id}: the request reached a route that is not ported yet"
        );
        compare(&case, &mut norm, &raw, closed, &booted.logs.since(mark));
        for (name, pattern) in captures_of(scenario, id) {
            let value = capture(&pattern, &raw)
                .unwrap_or_else(|| panic!("{scenario}/{id}: nothing captured for {name}"));
            captured.insert(name, value);
        }
        replayed += 1;
    }

    replayed
}
