use std::fmt;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};

use elysiandb::gocompat::json::{self, Value as JsonValue};
use elysiandb::gocompat::time::{self, Duration, Location, Month, Time};
use elysiandb::kv;
use elysiandb::log::{self, AttrValue, Level, Options, Query, Throttle, Value};
use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}

#[derive(Clone, Default)]
struct SafeBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for SafeBuffer {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(p);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SafeBuffer {
    fn string(&self) -> String {
        String::from_utf8_lossy(&self.0.lock()).into_owned()
    }
}

struct Cleanup;

impl Drop for Cleanup {
    fn drop(&mut self) {
        log::set_output(None, false);
        let _ = log::configure(&Options::default());
    }
}

fn setup(opts: Options) -> (SafeBuffer, Cleanup) {
    if let Err(err) = log::configure(&opts) {
        panic!("configure: {err}");
    }

    let buf = SafeBuffer::default();
    log::set_output(Some(Box::new(buf.clone())), false);

    (buf, Cleanup)
}

fn options(level: &str, format: &str, buffer_lines: i64) -> Options {
    Options {
        level: level.to_string(),
        format: format.to_string(),
        buffer_lines,
    }
}

fn lines(s: &str) -> Vec<&str> {
    s.trim_end_matches('\n').split('\n').collect()
}

fn count(q: Query) -> usize {
    log::logs(&q).entries.len()
}

#[test]
fn text_line_is_written_immediately_with_attributes() {
    let _serial = serial();
    let (buf, _cleanup) = setup(Options::default());

    log::for_component("storage").info(
        "storage loaded",
        kv!["folder" => "/data", "kv_keys" => 12, "duration_ms" => 1.5],
    );

    let out = buf.string();
    for want in [
        "level=INFO",
        "msg=\"storage loaded\"",
        "component=storage",
        "folder=/data",
        "kv_keys=12",
        "duration_ms=1.5",
    ] {
        assert!(out.contains(want), "missing {want:?} in {out:?}");
    }

    assert!(
        out.starts_with("time=") && out.ends_with('\n'),
        "unexpected line {out:?}"
    );
}

#[test]
fn component_comes_before_record_attributes() {
    let _serial = serial();
    let (buf, _cleanup) = setup(Options::default());

    log::for_component("http").error("request failed", kv!["status" => 500]);

    let out = buf.string();
    let component = out.find("component=http").expect("component");
    let status = out.find("status=500").expect("status");
    assert!(component <= status, "component should come first: {out:?}");
}

#[test]
fn client_values_are_quoted_and_escaped() {
    let _serial = serial();
    let (buf, _cleanup) = setup(Options::default());

    log::warn(
        "unknown command",
        kv!["command" => "FOO\x1b[31m bar\nlevel=ERROR"],
    );

    let out = buf.string();
    assert_eq!(
        out.matches('\n').count(),
        1,
        "a client value must not split the line: {out:?}"
    );
    assert!(
        !out.contains('\x1b'),
        "escape sequences must be escaped: {out:?}"
    );
    assert!(
        out.contains(r#"command="FOO\x1b[31m bar\nlevel=ERROR""#),
        "unexpected quoting: {out:?}"
    );
}

#[test]
fn json_format() {
    let _serial = serial();
    let (buf, _cleanup) = setup(options("", log::FORMAT_JSON, 0));

    let err = io::Error::other("bad password");
    log::for_component("security").warn(
        "login failed",
        kv!["user" => "admin", "attempts" => 3, "err" => err],
    );

    let out = buf.string();
    let line =
        json::unmarshal_value(out.as_bytes()).unwrap_or_else(|e| panic!("not JSON: {e} {out:?}"));

    let want = [
        ("level", JsonValue::String("WARN".to_string())),
        ("msg", JsonValue::String("login failed".to_string())),
        ("component", JsonValue::String("security".to_string())),
        ("user", JsonValue::String("admin".to_string())),
        ("attempts", JsonValue::Number(3.0)),
        ("err", JsonValue::String("bad password".to_string())),
    ];
    for (k, v) in want {
        let got = line.get(k).map(|g| json::marshal(g).expect("encode"));
        assert_eq!(
            got,
            Some(json::marshal(&v).expect("encode")),
            "{k} in {out:?}"
        );
    }

    let t = line.get("time").and_then(JsonValue::as_str).expect("time");
    if let Err(e) = time::parse(time::RFC3339_NANO, t) {
        panic!("time: {e}");
    }
}

#[test]
fn level_filtering() {
    let _serial = serial();
    let (buf, _cleanup) = setup(options("warn", "", 0));

    log::debug("d", kv![]);
    log::info("i", kv![]);
    log::warn("w", kv![]);
    log::error("e", kv![]);

    let out = buf.string();
    let got = lines(&out);
    assert!(
        got.len() == 2 && got[0].contains("msg=w") && got[1].contains("msg=e"),
        "unexpected lines {got:?}"
    );

    assert!(
        !log::enabled(Level::INFO) && log::enabled(Level::WARN),
        "enabled does not follow the level"
    );
}

#[test]
fn lines_keep_their_order() {
    let _serial = serial();
    let (buf, _cleanup) = setup(Options::default());

    for i in 0..500 {
        log::info("step", kv!["i" => i]);
    }

    let out = buf.string();
    for (i, line) in lines(&out).into_iter().enumerate() {
        assert!(
            line.ends_with(&format!(" i={i}")),
            "line {i} out of order: {line:?}"
        );
    }
}

#[test]
fn concurrent_logging_is_safe() {
    let _serial = serial();
    let (buf, _cleanup) = setup(options("", "", 100));

    std::thread::scope(|s| {
        for g in 0..8 {
            s.spawn(move || {
                for i in 0..200 {
                    log::info("concurrent", kv!["g" => g, "i" => i]);
                    let _ = log::logs(&Query {
                        limit: 10,
                        ..Query::default()
                    });
                }
            });
        }
    });

    let out = buf.string();
    assert_eq!(lines(&out).len(), 1600, "expected 1600 lines");
}

#[test]
fn configure_rejects_unknown_values() {
    let _serial = serial();

    assert!(
        log::configure(&options("verbose", "", 0)).is_err(),
        "expected an error for an unknown level"
    );
    assert!(
        log::configure(&options("", "xml", 0)).is_err(),
        "expected an error for an unknown format"
    );
    let _ = log::configure(&Options::default());
}

#[test]
fn parse_level() {
    let cases = [
        ("", Level::INFO),
        ("INFO", Level::INFO),
        ("debug", Level::DEBUG),
        ("warning", Level::WARN),
        (" error ", Level::ERROR),
    ];
    for (input, want) in cases {
        match log::parse_level(input) {
            Ok(got) if got == want => {}
            other => panic!("parse_level({input:?}) = {other:?}"),
        }
    }
}

#[test]
fn counts_warnings_and_errors() {
    let _serial = serial();
    let (_buf, _cleanup) = setup(Options::default());

    let (warnings, errors_count) = log::counts();
    log::warn("w", kv![]);
    log::error("e", kv![]);
    log::error("e", kv![]);
    log::info("i", kv![]);

    let (w, e) = log::counts();
    assert!(
        w - warnings == 1 && e - errors_count == 2,
        "unexpected counts {} {}",
        w - warnings,
        e - errors_count
    );
}

#[test]
fn buffer_is_disabled_with_zero_lines() {
    let _serial = serial();
    let (_buf, _cleanup) = setup(options("", "", 0));

    log::info("not kept", kv![]);

    assert!(!log::buffer_enabled(), "buffer should be disabled");
    let page = log::logs(&Query::default());
    assert!(
        page.entries.is_empty(),
        "unexpected entries {:?}",
        page.entries
    );
}

#[test]
fn buffer_keeps_the_last_lines_newest_first() {
    let _serial = serial();
    let (_buf, _cleanup) = setup(options("", "", 3));

    for i in 0..5 {
        log::for_component("boot").info("line", kv!["i" => i]);
    }

    let page = log::logs(&Query::default());
    assert!(
        page.size == 3 && page.capacity == 3 && page.entries.len() == 3,
        "unexpected page {page:?}"
    );

    assert!(
        page.entries[0].attrs.get("i") == Some(&AttrValue::Int(4))
            && page.entries[2].attrs.get("i") == Some(&AttrValue::Int(2)),
        "unexpected order {:?}",
        page.entries
    );

    assert!(
        page.entries[0].component == b"boot" && page.entries[0].level == "INFO",
        "unexpected entry {:?}",
        page.entries[0]
    );

    assert!(
        page.oldest_seq == page.entries[2].seq
            && page.latest_seq == page.entries[0].seq
            && page.buffer_since.is_some(),
        "unexpected bounds {page:?}"
    );
}

#[test]
fn buffer_queries() {
    let _serial = serial();
    let (_buf, _cleanup) = setup(options("", "", 100));

    log::for_component("http").info(
        "request",
        kv!["route" => "/api/{entity}", "request_id" => "r-1", "user" => "alice"],
    );
    log::for_component("http").error(
        "request failed",
        kv!["route" => "/api/{entity}", "request_id" => "r-2", "err" => "Timeout reached"],
    );
    log::for_component("security").warn("login failed", kv!["user" => "bob"]);
    log::for_component("audit").info("user created", kv!["actor" => "alice", "user" => "carol"]);
    log::for_component("storage").debug("flush done", kv![]);

    let got = count(Query {
        min_level: Level::WARN,
        ..Query::default()
    });
    assert_eq!(got, 2, "min level");

    let got = count(Query {
        component: b"http".to_vec(),
        ..Query::default()
    });
    assert_eq!(got, 2, "component");

    let got = count(Query {
        text: b"timeout".to_vec(),
        ..Query::default()
    });
    assert_eq!(got, 1, "text in attributes");

    let got = count(Query {
        text: b"LOGIN".to_vec(),
        ..Query::default()
    });
    assert_eq!(got, 1, "text in message");

    let got = count(Query {
        user: b"alice".to_vec(),
        ..Query::default()
    });
    assert_eq!(got, 2, "user or actor");

    let got = count(Query {
        request_id: b"r-2".to_vec(),
        ..Query::default()
    });
    assert_eq!(got, 1, "request id");

    let all = log::logs(&Query::default());
    let components: Vec<String> = all
        .components
        .iter()
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    assert_eq!(
        components.join(","),
        "audit,http,security",
        "debug lines below the level are not kept"
    );

    let newest = all.entries[0].seq;
    log::info("after", kv![]);
    let after = log::logs(&Query {
        after: newest,
        ..Query::default()
    });
    assert!(
        after.entries.len() == 1 && after.entries[0].message == "after",
        "after: {:?}",
        after.entries
    );

    let before = log::logs(&Query {
        before: newest,
        ..Query::default()
    });
    assert_eq!(before.entries.len(), 3, "before");

    let page = log::logs(&Query {
        limit: 2,
        ..Query::default()
    });
    assert!(page.entries.len() == 2 && page.has_more, "limit: {page:?}");

    let since = log::logs(&Query {
        since: Time::now().add(Duration::HOUR),
        ..Query::default()
    });
    assert_eq!(since.entries.len(), 0, "since");
}

#[test]
fn buffer_resize_keeps_the_newest_lines() {
    let _serial = serial();
    let (_buf, _cleanup) = setup(options("", "", 10));

    for i in 0..10 {
        log::info("line", kv!["i" => i]);
    }

    let _ = log::configure(&options("", "", 4));

    let page = log::logs(&Query::default());
    assert!(
        page.entries.len() == 4
            && page.entries[0].attrs.get("i") == Some(&AttrValue::Int(9))
            && page.entries[3].attrs.get("i") == Some(&AttrValue::Int(6)),
        "unexpected entries {:?}",
        page.entries
    );
}

#[test]
fn temporary_level_goes_back_by_itself() {
    let _serial = serial();
    let (buf, _cleanup) = setup(options("info", "", 10));

    let info = log::set_level_for(Level::DEBUG, Duration::MILLISECOND * 50);
    assert!(
        info.level == "debug" && info.configured == "info" && info.until.is_some(),
        "unexpected info {info:?}"
    );

    log::debug("visible", kv![]);

    let deadline = Instant::now() + StdDuration::from_secs(5);
    while !buf.string().contains("msg=\"log level restored\"") && Instant::now() < deadline {
        std::thread::sleep(StdDuration::from_millis(10));
    }

    assert!(
        buf.string().contains("msg=\"log level restored\""),
        "the restore should be logged: {:?}",
        buf.string()
    );

    let current = log::current_level();
    assert!(
        current.level == "info" && current.until.is_none(),
        "level not restored: {current:?}"
    );

    log::debug("hidden", kv![]);

    let out = buf.string();
    assert!(
        out.contains("msg=visible") && !out.contains("msg=hidden"),
        "unexpected output {out:?}"
    );
}

#[test]
fn setting_the_configured_level_cancels_the_override() {
    let _serial = serial();
    let (_buf, _cleanup) = setup(options("warn", "", 0));

    log::set_level_for(Level::DEBUG, Duration::HOUR);
    let info = log::set_level_for(Level::WARN, Duration(0));

    assert!(
        info.level == "warn" && info.until.is_none(),
        "unexpected info {info:?}"
    );
}

#[test]
fn throttle() {
    let th = Throttle::new(StdDuration::from_secs(60));
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = Arc::clone(&now);
    th.set_clock(move || *clock.lock());

    let (n, ok) = th.allow("k");
    assert!(ok && n == 0, "first call should pass");

    for i in 0..3 {
        let (_, ok) = th.allow("k");
        assert!(!ok, "call {i} should be suppressed");
    }

    let (_, ok) = th.allow("other");
    assert!(ok, "keys are independent");

    *now.lock() += StdDuration::from_secs(60);
    let (n, ok) = th.allow("k");
    assert!(ok && n == 3, "expected 3 suppressed, got {n} {ok}");

    th.allow("k");
    let n = th.reset("k");
    assert_eq!(n, 1, "reset should return the suppressed count");
    let (_, ok) = th.allow("k");
    assert!(ok, "a reset key passes again");
}

#[test]
fn with_suppressed() {
    let got = log::with_suppressed(kv!["a" => 1], 0);
    assert_eq!(got.len(), 1, "no attribute expected for 0");

    let got = log::with_suppressed(kv!["a" => 1], 4);
    assert!(
        got.len() == 2 && got[1].0 == "suppressed" && matches!(got[1].1, Value::Int(4)),
        "unexpected attributes"
    );
}

const FATAL_TEST: &str = "log_test::fatal_logs_and_exits_with_code_one";

#[test]
fn fatal_logs_and_exits_with_code_one() {
    if std::env::var_os("RUN_FATAL_HELPER").is_some_and(|v| v == "1") {
        let err = io::Error::other("kaboom");
        log::fatal("boom", kv!["err" => err]);
    }

    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([FATAL_TEST, "--exact", "--test-threads=1"])
        .env("RUN_FATAL_HELPER", "1")
        .output()
        .expect("child test run");

    assert_eq!(
        out.status.code(),
        Some(1),
        "expected exit code 1, got {:?}",
        out.status
    );

    let mut combined = out.stdout.clone();
    combined.extend_from_slice(&out.stderr);
    let combined = String::from_utf8_lossy(&combined);
    assert!(
        combined.contains("level=ERROR")
            && combined.contains("msg=boom")
            && combined.contains("err=kaboom"),
        "the fatal line should be written: {combined:?}"
    );
}

struct Stringer;

impl fmt::Display for Stringer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("from-stringer")
    }
}

#[test]
fn value_kinds_in_text() {
    let _serial = serial();
    let (buf, _cleanup) = setup(Options::default());

    let t = Time::from_date(2026, Month(11), 1, 12, 0, 0, 0, &Location::utc());
    let list = vec!["a".to_string(), "b".to_string()];
    let none: Option<&str> = None;
    let stringer = Stringer;
    log::for_component("kinds").with_group("ignored").info(
        "kinds",
        kv![
            "u" => 7u64,
            "f" => 1.25,
            "nan" => f64::NAN,
            "d" => Duration::MILLISECOND * 1500,
            "t" => t,
            "b" => true,
            "raw" => b"bytes".as_slice(),
            "list" => list,
            "none" => none,
            "str" => log::display(&stringer),
            "req" => log::group(kv!["method" => "GET", "status" => 200]),
            "" => "dropped",
        ],
    );

    let out = buf.string();
    for want in [
        "u=7",
        "f=1.25",
        "nan=NaN",
        "d=1.5s",
        "t=2026-11-01T12:00:00Z",
        "b=true",
        "raw=bytes",
        r#"list="[\"a\",\"b\"]""#,
        "none=null",
        "str=from-stringer",
        "req.method=GET",
        "req.status=200",
        "component=kinds",
    ] {
        assert!(out.contains(want), "missing {want:?} in {out:?}");
    }
    assert!(
        !out.contains("dropped"),
        "attributes without a key are dropped: {out:?}"
    );
}

#[test]
fn value_kinds_in_json() {
    let _serial = serial();
    let (buf, _cleanup) = setup(options("", log::FORMAT_JSON, 10));

    let list = vec!["a".to_string()];
    let none: Option<&str> = None;
    log::info(
        "kinds",
        kv![
            "u" => 7u64,
            "nan" => f64::INFINITY,
            "list" => list,
            "none" => none,
            "g" => log::group(kv!["k" => "v"]),
        ],
    );

    let out = buf.string();
    let line =
        json::unmarshal_value(out.as_bytes()).unwrap_or_else(|e| panic!("not JSON: {e} {out:?}"));

    assert!(
        line.get("u").and_then(JsonValue::as_f64) == Some(7.0)
            && line.get("nan").and_then(JsonValue::as_str) == Some("+Inf")
            && line.get("none").is_some_and(JsonValue::is_null)
            && line.get("g.k").and_then(JsonValue::as_str) == Some("v"),
        "unexpected line {out:?}"
    );
    let list = line.get("list").and_then(JsonValue::as_array);
    assert!(
        list.is_some_and(|l| l.len() == 1 && l[0].as_str() == Some("a")),
        "unexpected list {out:?}"
    );

    let page = log::logs(&Query::default());
    let entry = &page.entries[0];
    assert!(
        entry.attrs.get("g.k") == Some(&AttrValue::Str(b"v".to_vec()))
            && entry.attrs.get("u") == Some(&AttrValue::Uint(7)),
        "unexpected buffered attributes {:?}",
        entry.attrs
    );
}

#[test]
fn levels_are_colored_only_when_color_is_on() {
    let _serial = serial();
    let (buf, _cleanup) = setup(options("debug", "", 0));
    log::set_output(Some(Box::new(buf.clone())), true);

    log::debug("d", kv![]);
    log::info("i", kv![]);
    log::warn("w", kv![]);
    log::error("e", kv![]);

    let out = buf.string();
    for want in [
        "\x1b[35mDEBUG\x1b[0m",
        "\x1b[36mINFO\x1b[0m",
        "\x1b[33mWARN\x1b[0m",
        "\x1b[31mERROR\x1b[0m",
    ] {
        assert!(out.contains(want), "missing {want:?} in {out:?}");
    }

    let (plain, _plain_cleanup) = setup(Options::default());
    log::info("i", kv![]);
    assert!(
        !plain.string().contains('\x1b'),
        "no color expected: {:?}",
        plain.string()
    );

    assert_eq!(log::format(), log::FORMAT_TEXT, "unexpected format");
}

#[test]
fn throttle_forgets_old_keys_when_full() {
    let th = Throttle::new(StdDuration::from_secs(60));
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = Arc::clone(&now);
    th.set_clock(move || *clock.lock());

    for i in 0..10_000 {
        th.allow(format!("k{i}"));
    }

    let (_, ok) = th.allow("k1");
    assert!(!ok, "a recent key is still throttled");

    *now.lock() += StdDuration::from_secs(120);
    let (_, ok) = th.allow("new");
    assert!(ok, "a new key always passes");

    *now.lock() += StdDuration::from_secs(1);
    let (n, ok) = th.allow("k2");
    assert!(
        ok && n == 0,
        "old keys were forgotten when the throttle was full, got {n} {ok}"
    );
}
