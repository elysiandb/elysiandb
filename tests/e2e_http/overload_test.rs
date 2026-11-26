use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use elysiandb::api as api_storage;
use elysiandb::api::{Guard, ListQuery};
use elysiandb::boot;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::Value;
use elysiandb::hook;
use elysiandb::memory;
use elysiandb::metrics;
use elysiandb::security;

use crate::api_test::without_strict_schema;
use crate::harness::{
    ApiResponse, ApiServer, RACE_DETECTOR, marshal, obj, start_admin_session,
    start_tcp_admin_session, vs,
};

fn busy_size() -> (usize, usize) {
    if RACE_DETECTOR {
        return (2100, 1500);
    }

    (5000, 8000)
}

const BUSY_QUERY: &str = "/api/busy?search=nnn&countOnly=true";

fn seed_busy(s: &ApiServer) {
    let (docs, note) = busy_size();
    let mut start = 0;
    while start < docs {
        let mut list = Vec::with_capacity(250);
        for i in start..(start + 250).min(docs) {
            list.push(obj(vec![
                ("id", vs(&format!("b{i:05}"))),
                ("note", vs(&"n".repeat(note))),
                ("tags", Value::Array(vec![vs("n"), vs("m")])),
            ]));
        }
        let body = marshal(&Value::Array(list));
        s.must_do(200, "POST", "/api/busy", body, &[]);
        start += 250;
    }
}

struct HeavyLane {
    release: Arc<(Mutex<bool>, Condvar)>,
    done: Option<mpsc::Receiver<()>>,
}

impl HeavyLane {
    fn free(&mut self) {
        let Some(done) = self.done.take() else {
            return;
        };
        let (lock, cv) = &*self.release;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cv.notify_all();
        let _ = done.recv();
    }
}

impl Drop for HeavyLane {
    fn drop(&mut self) {
        self.free();
    }
}

fn hold_heavy_lane() -> HeavyLane {
    let (inside_tx, inside_rx) = mpsc::channel::<()>();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let calls = AtomicI64::new(0);
    let inside_tx = Mutex::new(Some(inside_tx));
    let gate = release.clone();
    let guard = Guard::new(
        None,
        Some(Box::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) + 1 < 3 {
                std::thread::sleep(Duration::from_millis(3));
                return false;
            }
            if calls.load(Ordering::SeqCst) == 3 {
                if let Some(tx) = inside_tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = tx.send(());
                }
                let (lock, cv) = &*gate;
                let mut open = lock.lock().unwrap_or_else(|e| e.into_inner());
                while !*open {
                    open = cv.wait(open).unwrap_or_else(|e| e.into_inner());
                }
            }
            false
        })),
    );

    std::thread::spawn(move || {
        api_storage::plan(ListQuery {
            entity: b"busy",
            search: b"nnn",
            count_only: true,
            guard: Some(&guard),
            ..ListQuery::default()
        });
        let _ = done_tx.send(());
    });

    if inside_rx.recv_timeout(Duration::from_secs(60)).is_err() {
        panic!("the holder never entered the heavy lane");
    }

    HeavyLane {
        release,
        done: Some(done_rx),
    }
}

impl ApiServer {
    fn async_do(&self, method: &'static str, path: &'static str) -> mpsc::Receiver<ApiResponse> {
        let (tx, rx) = mpsc::sync_channel(1);
        let s = self.anonymous_with_cookie();
        std::thread::spawn(move || {
            let out = match s.try_do(method, path, "", &[]) {
                Ok(resp) => ApiResponse {
                    status: resp.status,
                    body: resp.body,
                    headers: Default::default(),
                },
                Err(err) => ApiResponse {
                    status: 0,
                    body: err.to_string().into_bytes(),
                    headers: Default::default(),
                },
            };
            let _ = tx.send(out);
        });

        rx
    }

    fn anonymous_with_cookie(&self) -> ApiServer {
        let mut s = self.anonymous();
        s.cookie = self.cookie.clone();
        s
    }
}

fn wait_for_heavy_queue(n: i64) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while api_storage::heavy_waiting() != n {
        if Instant::now() > deadline {
            panic!(
                "expected {n} queued heavy queries, got {}",
                api_storage::heavy_waiting()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

struct SourcesReset;

impl Drop for SourcesReset {
    fn drop(&mut self) {
        metrics::set_sources(metrics::Sources::default());
    }
}

#[test]
fn full_heavy_queue_answers503() {
    let configure = |cfg: &mut Config| {
        without_strict_schema(cfg);
        cfg.api.query.workers = 1;
        cfg.api.query.max_queued = 1;
        cfg.api.query.queue_timeout_ms = -1;
    };
    let s = start_admin_session(Some(&configure));
    seed_busy(&s);
    let mut lane = hold_heavy_lane();

    let queued = s.async_do("GET", BUSY_QUERY);
    wait_for_heavy_queue(1);

    let busy = s.do_("GET", BUSY_QUERY, "", &[]);
    if busy.status != 503
        || busy.text() != r#"{"error":"server busy"}"#
        || busy.header("Retry-After") != "1"
    {
        panic!(
            "a request beyond the queue answers 503 server busy, got {} {} retry={:?}",
            busy.status,
            busy.text(),
            busy.header("Retry-After")
        );
    }

    lane.free();
    match queued.recv_timeout(Duration::from_secs(120)) {
        Ok(r) => {
            let (docs, _) = busy_size();
            if r.status != 200 || r.text() != format!(r#"{{"count":{docs}}}"#) {
                panic!(
                    "the queued request is served once the lane is free, got {} {}",
                    r.status,
                    r.text()
                );
            }
        }
        Err(_) => panic!("the queued request never finished"),
    }

    s.must_do(200, "GET", &format!("{BUSY_QUERY}&limit=1"), "", &[]);

    let exported = String::from_utf8_lossy(&metrics::prometheus()).into_owned();
    let counted = exported.lines().any(|line| {
        line.strip_prefix("elysiandb_heavy_rejected_total ")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| ('1'..='9').contains(&c))
    });
    if !counted {
        panic!("the rejection is counted in the Prometheus export");
    }

    metrics::set_sources(boot::monitoring_sources());
    let _sources = SourcesReset;

    let details = s
        .must_do(200, "GET", "/monitoring/details", "", &[])
        .object();
    let mut states = std::collections::BTreeMap::new();
    if let Some(Value::Array(indexes)) = details.get("indexes") {
        for idx in indexes {
            if idx.get("entity").and_then(Value::as_str) == Some("busy") {
                states.insert(
                    idx.get("index")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    idx.get("state")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                );
            }
        }
    }
    if states.get("search").is_none_or(String::is_empty) {
        panic!("the monitoring details list the indexes of busy, got {states:?}");
    }
}

#[test]
fn blocked_hook_query_stops_at_the_hook_timeout() {
    let configure = |cfg: &mut Config| {
        without_strict_schema(cfg);
        cfg.api.query.workers = 1;
        cfg.api.query.queue_timeout_ms = -1;
        cfg.api.hooks.timeout_ms = 300;
    };
    let s = start_admin_session(Some(&configure));
    seed_busy(&s);
    s.must_do(
        200,
        "POST",
        "/api/month",
        r#"{"id":"m1","label":"january"}"#,
        &[],
    );
    s.must_do(200, "POST", "/api/hook/month", r#"{"name":"total","event":"post_read","language":"javascript","enabled":true,"script":"function postRead(ctx){ ctx.entity.purchases = ctx.query(\"busy\", {\"tags\": {\"all\": \"n,m\"}}).length }"}"#, &[]);

    let mut lane = hold_heavy_lane();

    let start = Instant::now();
    let resp = s.must_do(200, "GET", "/api/month", "", &[]);
    let took = start.elapsed();
    if took > Duration::from_secs(2) {
        panic!(
            "a ctx.query blocked behind the heavy lane stops at the hook timeout, took {took:?}"
        );
    }
    if resp.text().contains("purchases") {
        panic!(
            "the stopped hook leaves the document as it was, got {} after {took:?}",
            resp.text()
        );
    }

    let mut last_error = String::new();
    for h in metrics::current_details().hooks {
        if h.entity == b"month" {
            last_error = h.last_error.clone();
        }
    }
    if !last_error.contains("hook script stopped after 300ms") {
        panic!("the hook error names the timeout, got {last_error:?}");
    }

    lane.free();
    let mut relaxed = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    relaxed.api.hooks.timeout_ms = 60000;
    globals::set_config(Some(Arc::new(relaxed)));

    let after = s.must_do(200, "GET", "/api/month", "", &[]).list();
    let (docs, _) = busy_size();
    if after[0].get("purchases").and_then(Value::as_f64) != Some(docs as f64) {
        panic!("once the lane is free the hook query is served, got {after:?}");
    }
}

fn hook_executions(entity: &str) -> u64 {
    let mut n = 0;
    for h in metrics::current_details().hooks {
        if h.entity == entity.as_bytes() {
            n += h.executions;
        }
    }

    n
}

#[test]
fn abandoned_request_stops_its_work() {
    let (s, addr) = start_tcp_admin_session(Some(&without_strict_schema));

    const DOCS: u64 = 300;
    let mut list = Vec::with_capacity(DOCS as usize);
    for i in 0..DOCS {
        list.push(obj(vec![
            ("id", vs(&format!("s{i:03}"))),
            ("n", Value::Number(i as f64)),
        ]));
    }
    let body = marshal(&Value::Array(list));
    s.must_do(200, "POST", "/api/spin", body, &[]);
    s.must_do(200, "POST", "/api/hook/spin", r#"{"name":"spin","event":"pre_read","language":"javascript","enabled":true,"script":"function preRead(ctx){ var t = Date.now(); while (Date.now() - t < 10) {} }"}"#, &[]);

    let mut conn = TcpStream::connect(&addr).unwrap_or_else(|e| panic!("{e}"));
    let _ = write!(
        conn,
        "GET /api/spin?filter%5Bn%5D%5Bgte%5D=0 HTTP/1.1\r\nHost: test\r\nCookie: {}={}\r\n\r\n",
        security::SESSION_COOKIE_NAME,
        s.cookie
    );

    std::thread::sleep(Duration::from_millis(300));
    if hook_executions("spin") == 0 {
        panic!("the request should be running its hooks");
    }
    drop(conn);

    std::thread::sleep(Duration::from_millis(200));
    let stopped = hook_executions("spin");
    std::thread::sleep(Duration::from_millis(700));
    let later = hook_executions("spin");
    if later != stopped || later >= DOCS {
        panic!(
            "the hooks of an abandoned request stop, {stopped} then {later} executions of {DOCS}"
        );
    }

    let page = s
        .must_do(200, "GET", "/api/spin?filter%5Bn%5D%5Blt%5D=2", "", &[])
        .ids();
    if page.join(",") != "s000,s001" {
        panic!("a client that waits is served as before, got {page:?}");
    }
}

struct MemoryReset;

impl Drop for MemoryReset {
    fn drop(&mut self) {
        memory::configure(0);
    }
}

#[test]
fn hook_queries_beyond_their_memory_share_answer503() {
    let s = start_admin_session(Some(&without_strict_schema));
    seed_busy(&s);
    s.must_do(
        200,
        "POST",
        "/api/month",
        r#"{"id":"m1","label":"january"}"#,
        &[],
    );
    s.must_do(200, "POST", "/api/hook/month", r#"{"name":"total","event":"post_read","language":"javascript","enabled":true,"bypass_acl":true,"script":"function postRead(ctx){ ctx.entity.purchases = ctx.query(\"busy\", {\"tags\": {\"all\": \"n,m\"}}).length }"}"#, &[]);

    memory::configure(4 << 20);
    let _reset = MemoryReset;
    hook::reset_query_cache();
    hook::reset_output_memo();

    for path in ["/api/month/m1", "/api/month"] {
        let resp = s.do_("GET", path, "", &[]);
        if resp.status != 503
            || !resp.text().contains("server busy")
            || resp.header("Retry-After") != "1"
        {
            panic!(
                "{path}: a hook query that the budget cannot hold answers 503 server busy, got {} {}",
                resp.status,
                resp.text()
            );
        }
    }

    memory::configure(0);
    hook::reset_query_cache();
    let doc = s.must_do(200, "GET", "/api/month/m1", "", &[]);
    let (docs, _) = busy_size();
    if !doc.text().contains(&format!(r#""purchases":{docs}"#)) {
        panic!(
            "without a budget the hook runs as before, got {}",
            doc.text()
        );
    }
}
