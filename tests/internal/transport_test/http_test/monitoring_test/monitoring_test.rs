use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration as StdDuration;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Value};
use elysiandb::gocompat::time::{self, Duration, Time};
use elysiandb::httpd::{Handler, Request, RequestCtx, handler};
use elysiandb::kv;
use elysiandb::log::{self, AttrValue, Query};
use elysiandb::metrics::{self, Moment};
use elysiandb::transport::http::monitoring;

use crate::tempdir::TempDir;

struct Setup {
    _dir: TempDir,
}

impl Drop for Setup {
    fn drop(&mut self) {
        log::set_output(None, false);
        let _ = log::configure(&log::Options::default());
        metrics::reset();
    }
}

fn setup(buffer_lines: i64) -> Setup {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.log.slow_request_ms = 250;
    globals::set_config(Some(Arc::new(cfg)));

    if let Err(e) = log::configure(&log::Options {
        level: "info".to_string(),
        buffer_lines,
        ..log::Options::default()
    }) {
        panic!("{e}");
    }
    log::set_output(Some(Box::new(std::io::sink())), false);
    metrics::reset();

    Setup { _dir: dir }
}

fn set_prometheus_token(token: &str) {
    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    cfg.monitoring.prometheus.token = token.to_string();
    globals::set_config(Some(Arc::new(cfg)));
}

fn call(
    h: &dyn Fn(&mut RequestCtx),
    method: &str,
    uri: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(uri);
    req.header.set_method(method);
    for (k, v) in headers {
        req.header.set(k, v);
    }
    if !body.is_empty() {
        req.set_body(body.as_bytes());
    }

    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    h(&mut ctx);

    ctx
}

fn get(h: fn(&mut RequestCtx), uri: &str) -> RequestCtx {
    call(&h, "GET", uri, "", &[])
}

fn decode(ctx: &RequestCtx) -> Value {
    match json::unmarshal_value(ctx.response.body_bytes()) {
        Ok(v) => v,
        Err(e) => panic!(
            "invalid JSON {:?}: {e}",
            String::from_utf8_lossy(ctx.response.body_bytes())
        ),
    }
}

static NULL: Value = Value::Null;

fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&NULL)
}

fn list(v: &Value, key: &str) -> Vec<Value> {
    field(v, key).as_array().cloned().unwrap_or_default()
}

fn int(v: &Value) -> i64 {
    v.as_f64().map(|f| f as i64).unwrap_or(-1)
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

#[test]
fn overview_controller() {
    let _s = setup(10);
    metrics::tick(Moment::now());

    let ctx = get(monitoring::overview_controller, "/monitoring/overview");
    let ct = String::from_utf8_lossy(ctx.response.header.content_type()).into_owned();
    assert!(
        ctx.response.status_code() == 200 && ct.starts_with("application/json"),
        "unexpected answer {} {ct}",
        ctx.response.status_code()
    );

    let body = decode(&ctx);
    for key in [
        "generated_at",
        "instance",
        "status",
        "traffic",
        "data",
        "persistence",
        "runtime",
        "connections",
        "logs",
    ] {
        assert!(body.get(key).is_some(), "missing {key} in {body:?}");
    }
}

#[test]
fn series_controller() {
    let _s = setup(10);
    let t0 = Moment::now();
    metrics::tick(t0.clone());
    metrics::tick(t0.add(Duration::SECOND));
    metrics::tick(t0.add(Duration::SECOND * 2));

    let body = decode(&get(monitoring::series_controller, "/monitoring/series"));
    let times = list(&body, "time");
    let http_rps = list(field(&body, "series"), "http_rps");
    assert!(
        text(field(&body, "range")) == "15m"
            && int(field(&body, "slow_request_ms")) == 250
            && times.len() == 2
            && http_rps.len() == 2,
        "unexpected series {body:?}"
    );

    let uri = format!("/monitoring/series?range=15m&since={}", int(&times[0]));
    let since = decode(&get(monitoring::series_controller, &uri));
    let times = list(&since, "time");
    assert_eq!(
        times.len(),
        1,
        "since should keep one point, got {}",
        times.len()
    );

    for uri in [
        "/monitoring/series?range=2h",
        "/monitoring/series?since=yesterday",
    ] {
        let ctx = get(monitoring::series_controller, uri);
        assert_eq!(
            ctx.response.status_code(),
            400,
            "{uri} should be refused, got {}",
            ctx.response.status_code()
        );
    }
}

#[test]
fn details_controller() {
    let _s = setup(10);
    metrics::observe_http("GET", "/api/{entity}", 200, StdDuration::from_millis(1), "");

    let body = decode(&get(monitoring::details_controller, "/monitoring/details"));

    assert!(body.get("routes").is_some(), "missing routes in {body:?}");
}

#[test]
fn logs_controller_filters() {
    let _s = setup(100);

    log::for_component("http").error(
        "request failed",
        kv!["route" => "/api/{entity}", "request_id" => "abc"],
    );
    log::for_component("audit").info("user created", kv!["actor" => "alice", "user" => "bob"]);
    log::for_component("security").warn("login failed", kv!["user" => "eve"]);

    let page = decode(&get(
        monitoring::logs_controller,
        "/monitoring/logs?level=warn",
    ));
    let entries = list(&page, "entries");
    assert!(
        entries.len() == 2 && text(field(&entries[0], "msg")) == "login failed",
        "unexpected warn page {entries:?}"
    );

    let page = decode(&get(
        monitoring::logs_controller,
        "/monitoring/logs?component=audit&limit=5",
    ));
    let entries = list(&page, "entries");
    assert!(
        entries.len() == 1 && text(field(field(&entries[0], "attrs"), "actor")) == "alice",
        "unexpected audit page {entries:?}"
    );

    let page = decode(&get(
        monitoring::logs_controller,
        "/monitoring/logs?request_id=abc",
    ));
    let entries = list(&page, "entries");
    assert_eq!(entries.len(), 1, "unexpected request id page {entries:?}");

    let page = decode(&get(
        monitoring::logs_controller,
        "/monitoring/logs?limit=1",
    ));
    let entries = list(&page, "entries");
    assert!(
        entries.len() == 1
            && field(&page, "has_more").as_bool() == Some(true)
            && list(&page, "components").len() == 3,
        "unexpected limited page {page:?}"
    );

    let latest = int(field(&page, "latest_seq"));
    log::info("newer", &[]);
    let uri = format!("/monitoring/logs?after={latest}");
    let page = decode(&get(monitoring::logs_controller, &uri));
    let entries = list(&page, "entries");
    assert!(
        entries.len() == 1 && text(field(&entries[0], "msg")) == "newer",
        "after should return the new line only: {entries:?}"
    );

    for uri in [
        "/monitoring/logs?level=loud",
        "/monitoring/logs?after=-1",
        "/monitoring/logs?since=now",
        "/monitoring/logs?limit=x",
    ] {
        let ctx = get(monitoring::logs_controller, uri);
        assert_eq!(
            ctx.response.status_code(),
            400,
            "{uri} should be refused, got {}",
            ctx.response.status_code()
        );
    }
}

#[test]
fn logs_controller_without_buffer() {
    let _s = setup(0);

    for h in [
        monitoring::logs_controller as fn(&mut RequestCtx),
        monitoring::logs_download_controller,
    ] {
        let ctx = get(h, "/monitoring/logs");
        assert_eq!(
            ctx.response.status_code(),
            404,
            "expected 404 without a buffer, got {}",
            ctx.response.status_code()
        );
    }
}

#[test]
fn logs_download() {
    let _s = setup(2000);

    for i in 0..1500i64 {
        log::info("line", kv!["i" => i]);
    }

    let ctx = get(
        monitoring::logs_download_controller,
        "/monitoring/logs/download",
    );
    let disposition =
        String::from_utf8_lossy(ctx.response.header.peek("Content-Disposition")).into_owned();
    assert!(
        disposition.contains("attachment"),
        "missing attachment header"
    );

    let body = String::from_utf8_lossy(ctx.response.body_bytes()).into_owned();
    let lines: Vec<&str> = body.trim().split('\n').collect();
    assert_eq!(
        lines.len(),
        1500,
        "the download must hold every matching line, got {}",
        lines.len()
    );

    let first = json::unmarshal_value(lines[0].as_bytes());
    let i = first
        .as_ref()
        .ok()
        .and_then(|v| v.get("attrs"))
        .and_then(|a| a.get("i"))
        .and_then(Value::as_f64);
    assert_eq!(
        i,
        Some(1499.0),
        "unexpected first line {:?} {:?}",
        lines[0],
        first.err()
    );
}

#[test]
fn log_level_controllers() {
    let _s = setup(100);

    let info = decode(&get(
        monitoring::get_log_level_controller,
        "/monitoring/logs/level",
    ));
    assert!(
        text(field(&info, "level")) == "info"
            && text(field(&info, "configured")) == "info"
            && field(&info, "until").is_null(),
        "unexpected level {info:?}"
    );

    let put = monitoring::put_log_level_controller;
    let info = decode(&call(
        &put,
        "PUT",
        "/monitoring/logs/level",
        r#"{"level":"debug","minutes":15}"#,
        &[],
    ));
    let until = time::parse(time::RFC3339_NANO, text(field(&info, "until")));
    let remaining = until.map(|u| u.sub(&Time::now()));
    assert!(
        text(field(&info, "level")) == "debug"
            && remaining
                .as_ref()
                .is_ok_and(|d| *d >= Duration::MINUTE * 14),
        "unexpected level {info:?}"
    );

    let audit = log::logs(&Query {
        component: b"audit".to_vec(),
        ..Query::default()
    })
    .entries;
    assert!(
        audit.len() == 1
            && audit[0].message == "log level changed"
            && audit[0].attrs.get("level") == Some(&AttrValue::Str(b"debug".to_vec())),
        "the change must be audited: {audit:?}"
    );

    let info = decode(&call(
        &put,
        "PUT",
        "/monitoring/logs/level",
        r#"{"level":"info"}"#,
        &[],
    ));
    assert!(
        text(field(&info, "level")) == "info" && field(&info, "until").is_null(),
        "the configured level resets the override: {info:?}"
    );

    for body in [
        r#"{"level":"debug"}"#,
        r#"{"level":"debug","minutes":2000}"#,
        r#"{"level":"loud","minutes":5}"#,
        r#"{"minutes":5}"#,
        "nope",
    ] {
        let ctx = call(&put, "PUT", "/monitoring/logs/level", body, &[]);
        assert_eq!(
            ctx.response.status_code(),
            400,
            "{body} should be refused, got {}",
            ctx.response.status_code()
        );
    }
}

#[test]
fn prometheus_token_auth() {
    let _s = setup(10);
    set_prometheus_token("s3cret");

    let admin_called = Arc::new(AtomicBool::new(false));
    let flag = admin_called.clone();
    let admin_auth = move |next: Handler| -> Handler {
        let flag = flag.clone();
        handler(move |ctx: &mut RequestCtx| {
            flag.store(true, Ordering::SeqCst);
            next(ctx);
        })
    };
    let h = monitoring::prometheus_auth(admin_auth, handler(monitoring::prometheus_controller));

    let ctx = call(&*h, "GET", "/metrics", "", &[]);
    assert_eq!(
        ctx.response.status_code(),
        401,
        "a missing token must be refused, got {}",
        ctx.response.status_code()
    );
    let ctx = call(
        &*h,
        "GET",
        "/metrics",
        "",
        &[("Authorization", "Bearer wrong")],
    );
    assert_eq!(
        ctx.response.status_code(),
        401,
        "a wrong token must be refused, got {}",
        ctx.response.status_code()
    );

    let ctx = call(
        &*h,
        "GET",
        "/metrics",
        "",
        &[("Authorization", "Bearer s3cret")],
    );
    let body = String::from_utf8_lossy(ctx.response.body_bytes()).into_owned();
    assert!(
        ctx.response.status_code() == 200 && body.contains("elysiandb_uptime_seconds"),
        "unexpected answer {} {body}",
        ctx.response.status_code()
    );
    assert!(
        !admin_called.load(Ordering::SeqCst),
        "with a token the admin credentials are not used"
    );

    set_prometheus_token("");
    call(&*h, "GET", "/metrics", "", &[]);
    assert!(
        admin_called.load(Ordering::SeqCst),
        "without a token the admin credentials are required"
    );
}
