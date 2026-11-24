use std::collections::BTreeMap;
use std::io;

use elysiandb::gocompat::json::{self, Value as JsonValue};
use elysiandb::gocompat::time::{self, Duration, Location, Month, Time};
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::kv;
use elysiandb::log::{self, Level, Options, Query, ToValue, Value};
use elysiandb::transport::http::monitoring;

use super::common::{self, str_field};
use super::{Capture, Restore, log_state};

elysiandb::json_struct! {
    struct Anonymous {
        a: i64 => "A",
    }
}

const TEXT_TIME_LAYOUT: &str = "2006-01-02T15:04:05.000Z07:00";

const MODES: [(&str, &str, bool); 3] = [
    ("text", log::FORMAT_TEXT, false),
    ("text_color", log::FORMAT_TEXT, true),
    ("json", log::FORMAT_JSON, false),
];

const D27: [(&str, &str, &str, &str); 10] = [
    ("value nil", "text", "value=<nil>", "value=null"),
    ("value nil", "text_color", "value=<nil>", "value=null"),
    (
        "value strings",
        "text",
        r#"value="[a b c]""#,
        r#"value="[\"a\",\"b c\"]""#,
    ),
    (
        "value strings",
        "text_color",
        r#"value="[a b c]""#,
        r#"value="[\"a\",\"b c\"]""#,
    ),
    (
        "value map",
        "text",
        r#"value="map[a:x b:1]""#,
        r#"value="{\"a\":\"x\",\"b\":1}""#,
    ),
    (
        "value map",
        "text_color",
        r#"value="map[a:x b:1]""#,
        r#"value="{\"a\":\"x\",\"b\":1}""#,
    ),
    (
        "value map",
        "json",
        r#""value":"map[a:x b:1]""#,
        r#""value":"{\"a\":\"x\",\"b\":1}""#,
    ),
    ("value struct", "text", "value={1}", r#"value="{\"A\":1}""#),
    (
        "value struct",
        "text_color",
        "value={1}",
        r#"value="{\"A\":1}""#,
    ),
    (
        "value struct",
        "json",
        r#""value":"{1}""#,
        r#""value":"{\"A\":1}""#,
    ),
];

type Call = (&'static str, Box<dyn Fn()>);

fn event(v: &dyn ToValue) {
    log::info("event", kv!["component" => "golden", "value" => v]);
}

fn value_call(name: &'static str, f: impl Fn() + 'static) -> Call {
    (name, Box::new(f))
}

fn calls() -> Vec<Call> {
    vec![
        value_call("value string", || event(&"plain")),
        value_call("value empty", || event(&"")),
        value_call("value spaces", || event(&"a b")),
        value_call("value equals", || event(&"a=b")),
        value_call("value quote", || event(&"say \"hi\"")),
        value_call("value backslash", || event(&"a\\b")),
        value_call("value newline", || event(&"a\nb")),
        value_call("value tab", || event(&"a\tb")),
        value_call("value control", || event(&"a\x01b")),
        value_call("value nbsp", || event(&"a\u{a0}b")),
        value_call("value zero-width", || event(&"a\u{200b}b")),
        value_call("value invalid-utf8", || event(&b"a\xffb".as_slice())),
        value_call("value emoji", || event(&"\u{1f600}")),
        value_call("value int", || event(&42)),
        value_call("value int64", || event(&i64::MIN)),
        value_call("value uint64", || event(&u64::MAX)),
        value_call("value float", || event(&1.5)),
        value_call("value float-big", || event(&1e21)),
        value_call("value float-small", || event(&1e-7)),
        value_call("value float-int", || event(&3.0)),
        value_call("value nan", || event(&f64::NAN)),
        value_call("value inf", || event(&f64::INFINITY)),
        value_call("value bool", || event(&true)),
        value_call("value nil", || event(&Value::Nil)),
        value_call("value duration", || event(&(Duration::MILLISECOND * 1500))),
        value_call("value time", || {
            event(&Time::from_date(
                2026,
                Month(11),
                23,
                18,
                39,
                1,
                120_000_000,
                &Location::utc(),
            ))
        }),
        value_call("value error", || event(&io::Error::other("boom \"x\""))),
        value_call("value strings", || {
            event(&vec!["a".to_string(), "b c".to_string()])
        }),
        value_call("value map", || {
            let mut m: BTreeMap<String, JsonValue> = BTreeMap::new();
            m.insert("b".to_string(), JsonValue::Int(1));
            m.insert("a".to_string(), JsonValue::String("x".to_string()));
            event(&m)
        }),
        value_call("value bytes", || event(&b"raw".to_vec())),
        value_call("value struct", || event(&log::json(&Anonymous { a: 1 }))),
        value_call("duplicates", || {
            log::info("dup", kv!["component" => "golden", "k" => 1, "k" => 2])
        }),
        value_call("odd args", || {
            log::info("odd", kv!["component" => "golden", "!BADKEY" => "lonely"])
        }),
        value_call("no component", || log::warn("bare message", kv![])),
        value_call("message quoting", || {
            log::info(
                "msg with \"quotes\" and = sign",
                kv!["component" => "golden"],
            )
        }),
        value_call("for component", || {
            log::for_component("storage").info("from logger", kv!["n" => 1])
        }),
        value_call("error level", || {
            log::error(
                "failed",
                kv!["component" => "golden", "err" => io::Error::other("disk full")],
            )
        }),
        value_call("debug hidden", || {
            log::debug("hidden", kv!["component" => "golden"])
        }),
        value_call("group", || {
            log::info(
                "grouped",
                kv!["component" => "golden", "g" => log::group(kv!["a" => 1, "b" => "x"])],
            )
        }),
    ]
}

const QUERY_URIS: [(&str, &str); 9] = [
    ("buffer all", "/monitoring/logs?level=info&limit=100"),
    ("buffer limit 3", "/monitoring/logs?level=info&limit=3"),
    ("buffer warn+", "/monitoring/logs?level=warn&limit=100"),
    (
        "buffer component a",
        "/monitoring/logs?level=info&component=a&limit=100",
    ),
    ("buffer text", "/monitoring/logs?level=info&q=line&limit=2"),
    (
        "buffer user",
        "/monitoring/logs?level=info&user=ann&limit=100",
    ),
    (
        "buffer request id",
        "/monitoring/logs?level=info&request_id=r-1&limit=100",
    ),
    (
        "buffer before",
        "/monitoring/logs?level=info&before=10&limit=100",
    ),
    (
        "buffer after",
        "/monitoring/logs?level=info&after=8&limit=100",
    ),
];

fn queries() -> Vec<(&'static str, Query)> {
    let q = |f: &dyn Fn(&mut Query)| {
        let mut q = Query::default();
        f(&mut q);
        q
    };
    vec![
        ("buffer all", q(&|q| q.limit = 100)),
        ("buffer limit 3", q(&|q| q.limit = 3)),
        (
            "buffer warn+",
            q(&|q| {
                q.min_level = Level::WARN;
                q.limit = 100;
            }),
        ),
        (
            "buffer component a",
            q(&|q| {
                q.component = b"a".to_vec();
                q.limit = 100;
            }),
        ),
        (
            "buffer text",
            q(&|q| {
                q.text = b"line".to_vec();
                q.limit = 2;
            }),
        ),
        (
            "buffer user",
            q(&|q| {
                q.user = b"ann".to_vec();
                q.limit = 100;
            }),
        ),
        (
            "buffer request id",
            q(&|q| {
                q.request_id = b"r-1".to_vec();
                q.limit = 100;
            }),
        ),
        (
            "buffer before",
            q(&|q| {
                q.before = 10;
                q.limit = 100;
            }),
        ),
        (
            "buffer after",
            q(&|q| {
                q.after = 8;
                q.limit = 100;
            }),
        ),
    ]
}

fn mask_log_times(out: &str) -> String {
    let mut masked = String::with_capacity(out.len());
    for line in out.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("time=") {
            let end = rest.find(' ').unwrap_or(rest.len());
            if let Err(e) = time::parse(TEXT_TIME_LAYOUT, &rest[..end]) {
                panic!("text time {:?}: {e}", &rest[..end]);
            }
            masked.push_str("time=<T>");
            masked.push_str(&rest[end..]);
        } else if let Some(rest) = line.strip_prefix("{\"time\":\"") {
            let end = rest.find('"').expect("closing quote of the time");
            if let Err(e) = time::parse(time::RFC3339_NANO, &rest[..end]) {
                panic!("JSON time {:?}: {e}", &rest[..end]);
            }
            masked.push_str("{\"time\":\"<T>\"");
            masked.push_str(&rest[end + 1..]);
        } else {
            masked.push_str(line);
        }
    }
    masked
}

fn mask_page_times(page: &str) -> String {
    let mut masked = String::with_capacity(page.len());
    let mut rest = page;
    loop {
        let next = ["\"time\":\"", "\"buffer_since\":\""]
            .iter()
            .filter_map(|p| rest.find(p).map(|i| (i, p.len())))
            .min();
        let Some((at, len)) = next else {
            masked.push_str(rest);
            return masked;
        };
        masked.push_str(&rest[..at + len]);
        rest = &rest[at + len..];
        let end = rest.find('"').expect("closing quote of the time");
        if let Err(e) = time::parse(time::RFC3339_NANO, &rest[..end]) {
            panic!("page time {:?}: {e}", &rest[..end]);
        }
        masked.push_str("<T>\"");
        rest = &rest[end + 1..];
    }
}

fn expected_line(call: &str, mode: &str, golden: &str, used: &mut [bool]) -> String {
    let mut expected = golden.to_string();
    for (i, (c, m, go, rust)) in D27.iter().enumerate() {
        if *c != call || *m != mode {
            continue;
        }
        assert_eq!(
            golden.matches(go).count(),
            1,
            "{call} {mode}: the Go rendering {go:?} is not in {golden:?}"
        );
        expected = expected.replace(go, rust);
        used[i] = true;
    }
    expected
}

#[test]
fn log_lines_and_buffer_pages_match_go() {
    let _state = log_state();
    let _restore = Restore;

    let rows = common::read_jsonl("ops/logs/cases.jsonl");
    let calls = calls();
    let queries = queries();
    assert_eq!(rows.len(), calls.len() + queries.len());
    assert_eq!(rows.len(), 48);

    let mut used = [false; D27.len()];
    let mut checked = 0;
    let capture = Capture::default();

    for (index, (name, call)) in calls.iter().enumerate() {
        let row = &rows[index];
        assert_eq!(str_field(row, "call"), *name);

        for (key, format, color) in MODES {
            let opts = Options {
                level: "info".to_string(),
                format: format.to_string(),
                buffer_lines: 100,
            };
            log::configure(&opts).expect("configure");
            log::set_output(Some(Box::new(capture.clone())), color);
            call();
            let got = mask_log_times(&capture.take());
            let want = expected_line(name, key, str_field(row, key), &mut used);
            assert_eq!(got, want, "{name} {key}");

            if index == 0 && key == "text" {
                assert_eq!(
                    log::logs(&Query::default()).latest_seq,
                    1,
                    "the buffer recorded lines before this test: the sequence numbers would not match"
                );
            }
        }
        checked += 1;
    }

    let opts = Options {
        level: "debug".to_string(),
        format: log::FORMAT_TEXT.to_string(),
        buffer_lines: 8,
    };
    log::configure(&opts).expect("configure");
    log::set_output(Some(Box::new(capture.clone())), false);
    for i in 0..12 {
        match i % 4 {
            0 => log::debug("line", kv!["component" => "a", "i" => i]),
            1 => log::info("line", kv!["component" => "b", "i" => i, "user" => "ann"]),
            2 => log::warn(
                "line",
                kv!["component" => "a", "i" => i, "request_id" => "r-1"],
            ),
            _ => log::error("line", kv!["component" => "c", "i" => i]),
        }
    }

    for (offset, (name, q)) in queries.iter().enumerate() {
        let row = &rows[calls.len() + offset];
        assert_eq!(str_field(row, "call"), *name);
        let page = json::marshal(&log::logs(q)).expect("encode the page");
        let page = String::from_utf8(page).expect("page is UTF-8");
        assert_eq!(mask_page_times(&page), str_field(row, "page"), "{name}");
        checked += 1;
    }

    let mut served = 0;
    for (offset, (name, uri)) in QUERY_URIS.iter().enumerate() {
        let row = &rows[calls.len() + offset];
        assert_eq!(str_field(row, "call"), *name);
        let mut req = Request::new();
        req.set_request_uri(uri);
        req.header.set_method("GET");
        let mut ctx = RequestCtx::new();
        ctx.init(&req, None);
        monitoring::logs_controller(&mut ctx);
        assert_eq!(ctx.response.status_code(), 200, "{uri}");
        assert_eq!(
            ctx.response.header.content_type(),
            b"application/json",
            "{uri}"
        );
        let page = String::from_utf8(ctx.response.body_bytes().to_vec()).expect("page is UTF-8");
        assert_eq!(mask_page_times(&page), str_field(row, "page"), "{uri}");
        served += 1;
    }
    assert_eq!(served, queries.len());

    assert!(used.iter().all(|u| *u), "every D27 rendering is checked");
    assert_eq!(checked, rows.len());
}
