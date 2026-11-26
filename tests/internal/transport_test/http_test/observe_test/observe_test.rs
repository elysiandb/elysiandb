use std::io::Write;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::time::Duration;
use elysiandb::httpd::{Handler, Request, RequestCtx, STATUS_SERVICE_UNAVAILABLE, handler};
use elysiandb::httprouter::Router;
use elysiandb::kv;
use elysiandb::log::{self, AttrValue, Entry, Level, Query};
use elysiandb::metrics::{self, Moment};
use elysiandb::security;
use elysiandb::transport::http::observe;

use crate::tempdir::TempDir;

struct Setup {
    handler: Handler,
    _dir: TempDir,
}

impl Drop for Setup {
    fn drop(&mut self) {
        log::set_output(None, false);
        let _ = log::configure(&log::Options::default());
        metrics::reset();
    }
}

fn setup(mutate: Option<&dyn Fn(&mut Config)>) -> Setup {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.log.slow_request_ms = 50;
    if let Some(m) = mutate {
        m(&mut cfg);
    }
    globals::set_config(Some(Arc::new(cfg)));

    if let Err(e) = log::configure(&log::Options {
        level: "debug".to_string(),
        buffer_lines: 1000,
        ..log::Options::default()
    }) {
        panic!("{e}");
    }
    log::set_output(Some(Box::new(std::io::sink())), false);
    metrics::reset();
    observe::reset_throttles();

    let mut r = Router::new();
    r.save_matched_route_path = true;
    r.panic_handler = Some(observe::panic_handler());

    r.get(
        "/ok/{entity}",
        handler(|ctx: &mut RequestCtx| ctx.set_body_string("ok")),
    );
    r.get(
        "/fail",
        handler(|ctx: &mut RequestCtx| {
            observe::set_error(ctx, &"disk on fire");
            ctx.set_status_code(500);
        }),
    );
    r.get("/panic", handler(|_ctx: &mut RequestCtx| panic!("kaboom")));
    r.get(
        "/busy",
        handler(|ctx: &mut RequestCtx| {
            observe::set_rejected(ctx, "server busy");
            ctx.set_status_code(STATUS_SERVICE_UNAVAILABLE);
        }),
    );
    r.get(
        "/stream",
        handler(|ctx: &mut RequestCtx| {
            ctx.set_body_stream_writer(|w| {
                let _ = w.write_all(b"streamed");
            });
        }),
    );
    r.get(
        "/slow",
        handler(|_ctx: &mut RequestCtx| std::thread::sleep(StdDuration::from_millis(60))),
    );
    r.get(
        "/denied",
        handler(|ctx: &mut RequestCtx| ctx.set_status_code(401)),
    );
    r.get(
        "/forbidden",
        handler(|ctx: &mut RequestCtx| {
            ctx.set_user_value(security::USERNAME_KEY, b"bob".to_vec());
            ctx.set_status_code(403);
        }),
    );
    r.get(
        "/missing",
        handler(|ctx: &mut RequestCtx| ctx.set_status_code(404)),
    );
    r.get("/monitoring/overview", handler(|_ctx: &mut RequestCtx| {}));
    r.get(
        "/api/security/me",
        handler(|ctx: &mut RequestCtx| ctx.set_status_code(401)),
    );

    Setup {
        handler: observe::middleware(r.into_handler()),
        _dir: dir,
    }
}

fn call(h: &Handler, method: &str, uri: &str, headers: &[(&str, &str)]) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(uri);
    req.header.set_method(method);
    for (k, v) in headers {
        req.header.set(k, v);
    }

    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    h(&mut ctx);

    ctx
}

fn entries(q: Query) -> Vec<Arc<Entry>> {
    log::logs(&q).entries
}

fn min_level(level: Level) -> Query {
    Query {
        min_level: level,
        ..Query::default()
    }
}

fn header(ctx: &RequestCtx, name: &str) -> String {
    String::from_utf8_lossy(ctx.response.header.peek(name)).into_owned()
}

fn str_attr(e: &Entry, key: &str) -> Option<String> {
    match e.attrs.get(key) {
        Some(AttrValue::Str(s)) => Some(String::from_utf8_lossy(s).into_owned()),
        _ => None,
    }
}

#[test]
fn request_id_is_kept_or_generated() {
    let s = setup(None);
    let h = &s.handler;

    let ctx = call(
        h,
        "GET",
        "/ok/books",
        &[(observe::REQUEST_ID_HEADER, "client-id_1.2")],
    );
    let got = header(&ctx, observe::REQUEST_ID_HEADER);
    assert_eq!(
        got, "client-id_1.2",
        "a valid client request id must be kept, got {got:?}"
    );

    let long = "a".repeat(65);
    for invalid in ["has space", long.as_str(), "semi;colon"] {
        let ctx = call(
            h,
            "GET",
            "/ok/books",
            &[(observe::REQUEST_ID_HEADER, invalid)],
        );
        let got = header(&ctx, observe::REQUEST_ID_HEADER);
        assert!(
            !got.is_empty() && got != invalid,
            "an invalid request id must be replaced, got {got:?} for {invalid:?}"
        );
    }

    let first = header(
        &call(h, "GET", "/ok/books", &[]),
        observe::REQUEST_ID_HEADER,
    );
    let second = header(
        &call(h, "GET", "/ok/books", &[]),
        observe::REQUEST_ID_HEADER,
    );
    assert!(
        !first.is_empty() && first != second,
        "generated ids must be unique: {first:?} {second:?}"
    );
}

#[test]
fn server_error_is_logged_once_with_its_error() {
    let s = setup(None);

    let ctx = call(
        &s.handler,
        "GET",
        "/fail",
        &[(observe::REQUEST_ID_HEADER, "req-9")],
    );

    let got = entries(min_level(Level::ERROR));
    assert_eq!(got.len(), 1, "expected one error line, got {got:?}");

    let e = &got[0];
    assert!(
        e.message == "request failed"
            && e.component == b"http"
            && str_attr(e, "err").as_deref() == Some("disk on fire")
            && str_attr(e, "route").as_deref() == Some("/fail")
            && e.attrs.get("status") == Some(&AttrValue::Int(500))
            && str_attr(e, "request_id").as_deref() == Some("req-9"),
        "unexpected entry {e:?}"
    );

    assert_eq!(
        ctx.response.status_code(),
        500,
        "unexpected status {}",
        ctx.response.status_code()
    );
}

#[test]
fn panic_is_recovered_and_logged_once() {
    let s = setup(None);
    observe::install_panic_hook();

    let ctx = call(&s.handler, "GET", "/panic", &[]);
    assert_eq!(
        ctx.response.status_code(),
        500,
        "a panic must answer 500, got {}",
        ctx.response.status_code()
    );

    let got = entries(min_level(Level::ERROR));
    assert!(
        got.len() == 1
            && got[0].message == "handler panic"
            && str_attr(&got[0], "panic").as_deref() == Some("kaboom"),
        "expected one handler panic line, got {got:?}"
    );

    let stack = str_attr(&got[0], "stack").unwrap_or_default();
    assert!(!stack.trim().is_empty(), "the stack must be logged");
}

#[test]
fn unauthorized_is_a_warning_only_with_credentials() {
    let s = setup(None);
    let h = &s.handler;

    call(h, "GET", "/denied", &[]);
    let got = entries(min_level(Level::WARN));
    assert!(
        got.is_empty(),
        "a request without credentials is not a security warning: {got:?}"
    );

    call(h, "GET", "/denied", &[("Authorization", "Bearer nope")]);
    call(h, "GET", "/denied", &[("Authorization", "Bearer nope")]);

    let got = entries(min_level(Level::WARN));
    assert!(
        got.len() == 1
            && got[0].message == "unauthorized request"
            && got[0].component == b"security",
        "expected one throttled warning, got {got:?}"
    );

    let cookie = format!("{}=expired", security::SESSION_COOKIE_NAME);
    call(h, "GET", "/api/security/me", &[("Cookie", cookie.as_str())]);
    let got = entries(min_level(Level::WARN));
    assert_eq!(
        got.len(),
        1,
        "the Admin UI session check must stay quiet: {got:?}"
    );
}

#[test]
fn forbidden_is_a_warning_with_the_user() {
    let s = setup(None);

    call(&s.handler, "GET", "/forbidden", &[]);

    let got = entries(min_level(Level::WARN));
    assert!(
        got.len() == 1
            && got[0].message == "forbidden request"
            && str_attr(&got[0], "user").as_deref() == Some("bob"),
        "unexpected entries {got:?}"
    );
}

#[test]
fn client_errors_are_debug() {
    let s = setup(None);

    call(&s.handler, "GET", "/missing", &[]);
    call(&s.handler, "GET", "/nowhere", &[]);

    let got = entries(min_level(Level::INFO));
    assert!(
        got.is_empty(),
        "4xx answers are not logged above debug: {got:?}"
    );

    let got = entries(Query {
        min_level: Level::DEBUG,
        text: b"client error".to_vec(),
        ..Query::default()
    });
    assert_eq!(got.len(), 2, "expected two debug lines, got {got:?}");

    assert_eq!(
        str_attr(&got[0], "route").as_deref(),
        Some("unmatched"),
        "an unknown path is reported as unmatched, got {:?}",
        got[0]
    );
}

#[test]
fn slow_request_is_a_warning() {
    let s = setup(None);

    call(&s.handler, "GET", "/slow", &[]);

    let got = entries(min_level(Level::WARN));
    assert!(
        got.len() == 1
            && got[0].message == "slow request"
            && str_attr(&got[0], "route").as_deref() == Some("/slow"),
        "unexpected entries {got:?}"
    );
}

#[test]
fn slow_request_can_be_disabled() {
    let s = setup(Some(&|cfg: &mut Config| cfg.log.slow_request_ms = 0));

    call(&s.handler, "GET", "/slow", &[]);

    let got = entries(min_level(Level::WARN));
    assert!(
        got.is_empty(),
        "slowRequestMs: 0 disables the warning: {got:?}"
    );
}

#[test]
fn access_log() {
    let s = setup(Some(&|cfg: &mut Config| cfg.log.access_log = true));

    call(&s.handler, "GET", "/ok/books", &[]);
    call(&s.handler, "GET", "/monitoring/overview", &[]);

    let got = entries(min_level(Level::INFO));
    assert!(
        got.len() == 1
            && got[0].message == "request"
            && str_attr(&got[0], "route").as_deref() == Some("/ok/{entity}")
            && got[0].attrs.get("status") == Some(&AttrValue::Int(200)),
        "expected one access line for the API request only, got {got:?}"
    );
}

#[test]
fn access_log_leaves_streamed_responses_streamed() {
    let s = setup(Some(&|cfg: &mut Config| cfg.log.access_log = true));

    let ctx = call(&s.handler, "GET", "/stream", &[]);

    assert!(
        ctx.response.is_body_stream(),
        "the access log must not turn a streamed response into a buffered one"
    );

    let got = entries(min_level(Level::INFO));
    assert!(
        got.len() == 1
            && got[0].attrs.get("streamed") == Some(&AttrValue::Bool(true))
            && str_attr(&got[0], "route").as_deref() == Some("/stream"),
        "expected one access line marked as streamed, got {got:?}"
    );
}

#[test]
fn no_access_log_by_default() {
    let s = setup(None);

    call(&s.handler, "GET", "/ok/books", &[]);

    let got = entries(min_level(Level::INFO));
    assert!(
        got.is_empty(),
        "no info line per request by default: {got:?}"
    );
}

#[test]
fn monitoring_requests_are_not_measured() {
    let s = setup(None);
    let t0 = Moment::now();

    metrics::tick(t0.clone());
    call(&s.handler, "GET", "/monitoring/overview", &[]);
    call(&s.handler, "GET", "/ok/books", &[]);
    metrics::tick(t0.add(Duration::SECOND));

    let routes = metrics::current_details().routes.since_startup;
    assert!(
        routes.len() == 1 && routes[0].route == b"/ok/{entity}",
        "monitoring requests must not be counted: {routes:?}"
    );
}

#[test]
fn kind() {
    let cases = [
        ("GET", "/kv/{key}", metrics::KIND_KV_GET),
        ("GET", "/kv/mget", metrics::KIND_KV_GET),
        ("PUT", "/kv/{key}", metrics::KIND_KV_SET),
        ("DELETE", "/kv/{key}", metrics::KIND_KV_DELETE),
        ("POST", "/api/query", metrics::KIND_DOC_QUERY),
        ("GET", "/api/{entity}", metrics::KIND_DOC_READ),
        ("GET", "/api/{entity}/{id}", metrics::KIND_DOC_READ),
        ("POST", "/api/{entity}", metrics::KIND_DOC_WRITE),
        ("DELETE", "/api/{entity}/{id}", metrics::KIND_DOC_WRITE),
        ("POST", "/api/tx/{txId}/commit", metrics::KIND_DOC_WRITE),
        ("GET", "/health", ""),
        ("GET", "/api/backup", ""),
    ];

    for (method, route, want) in cases {
        let got = observe::kind(method, route);
        assert_eq!(got, want, "Kind({method} {route}) = {got:?}, want {want:?}");
    }
}

#[test]
fn excluded() {
    for (path, want) in [
        ("/health", true),
        ("/metrics", true),
        ("/admin", true),
        ("/admin/assets/app.js", true),
        ("/monitoring/logs", true),
        ("/administrators", false),
        ("/api/monitoring", false),
        ("/api/books", false),
    ] {
        let got = observe::excluded(path.as_bytes());
        assert_eq!(got, want, "Excluded({path:?}) = {got}");
    }
}

#[test]
fn audit_carries_actor_and_request() {
    let _s = setup(None);

    let mut req = Request::new();
    req.set_request_uri("/x");
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx.set_user_value(security::USERNAME_KEY, b"alice".to_vec());

    observe::audit(&ctx, "schema changed", kv!["entity" => "books"]);

    let got = entries(Query {
        component: b"audit".to_vec(),
        ..Query::default()
    });
    assert!(
        got.len() == 1
            && got[0].message == "schema changed"
            && str_attr(&got[0], "actor").as_deref() == Some("alice")
            && str_attr(&got[0], "source").as_deref() == Some("http")
            && str_attr(&got[0], "entity").as_deref() == Some("books"),
        "unexpected audit entry {got:?}"
    );
}

#[test]
fn rejected_requests_are_grouped_warnings() {
    let s = setup(None);

    for _ in 0..3 {
        let ctx = call(&s.handler, "GET", "/busy", &[]);
        assert_eq!(
            ctx.response.status_code(),
            STATUS_SERVICE_UNAVAILABLE,
            "the status is kept, got {}",
            ctx.response.status_code()
        );
    }

    let errs = entries(min_level(Level::ERROR));
    assert!(
        errs.is_empty(),
        "a rejected request is not an error, got {errs:?}"
    );

    let got = entries(min_level(Level::WARN));
    assert!(
        got.len() == 1
            && got[0].message == "request rejected"
            && str_attr(&got[0], "reason").as_deref() == Some("server busy")
            && str_attr(&got[0], "route").as_deref() == Some("/busy"),
        "one grouped warning is expected, got {got:?}"
    );
}
