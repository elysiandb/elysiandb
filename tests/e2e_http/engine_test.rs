use std::time::Duration;

use elysiandb::configuration::Config;
use elysiandb::gocompat::json::{self, Value};

use crate::api_test::without_strict_schema;
use crate::harness::{marshal, obj, start_admin_session, vn, vs};

fn scan_int(text: &str) -> i64 {
    let digits: String = text
        .trim_start()
        .chars()
        .enumerate()
        .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
        .map(|(_, c)| c)
        .collect();
    digits.parse().unwrap_or(0)
}

#[test]
fn request_deadline_answers503() {
    let configure = |cfg: &mut Config| {
        cfg.api.schema.strict = false;
        cfg.api.query.timeout_ms = 1;
        cfg.api.query.workers = 1;
    };
    let s = start_admin_session(Some(&configure));

    let mut list = Vec::with_capacity(20000);
    for i in 0..20000 {
        list.push(obj(vec![
            ("id", vs(&format!("d{i:05}"))),
            ("note", vs(&"n".repeat(40))),
            ("k", vn(f64::from(i % 7))),
        ]));
    }
    let body = marshal(&Value::Array(list));
    s.must_do(200, "POST", "/api/big", body, &[]);

    let mut expired = false;
    let mut i = 0;
    while i < 5 && !expired {
        let resp = s.do_(
            "GET",
            "/api/big?filter%5Bnote%5D%5Bneq%5D=x&search=nnn",
            "",
            &[],
        );
        if resp.status == 503 {
            expired = true;
            if resp.text() != r#"{"error":"request deadline exceeded"}"# {
                panic!("unexpected body {}", resp.text());
            }
        }
        i += 1;
    }

    if !expired {
        panic!("a scan longer than api.query.timeoutMs should answer 503");
    }

    let page = s
        .must_do(200, "GET", "/api/big?limit=3&offset=100", "", &[])
        .ids();
    if page.join(",") != "d00100,d00101,d00102" {
        panic!("index-only plans are not slowed down, got {page:?}");
    }
}

#[test]
fn cached_lists_follow_included_documents() {
    let s = start_admin_session(Some(&without_strict_schema));

    s.must_do(
        200,
        "POST",
        "/api/writer",
        r#"{"id":"w1","name":"Herbert"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/novel",
        r#"{"id":"n1","title":"Dune","writer":{"@entity":"writer","id":"w1"}}"#,
        &[],
    );

    let first = s.must_do(200, "GET", "/api/novel?includes=writer", "", &[]);
    if !first.text().contains("Herbert") {
        panic!("expected the included writer, got {}", first.text());
    }
    let hit = s.must_do(200, "GET", "/api/novel?includes=writer", "", &[]);
    if hit.header("X-Elysian-Cache") != "HIT" {
        panic!("the second read should come from the cache");
    }

    s.must_do(
        200,
        "PUT",
        "/api/writer/w1",
        r#"{"name":"Frank Herbert"}"#,
        &[],
    );

    let after = s.must_do(200, "GET", "/api/novel?includes=writer", "", &[]);
    if after.header("X-Elysian-Cache") != "MISS" || !after.text().contains("Frank Herbert") {
        panic!(
            "a cached response that includes a changed document is invalidated, got {} {}",
            after.header("X-Elysian-Cache"),
            after.text()
        );
    }
}

#[test]
fn stats_count_every_request() {
    let configure = |cfg: &mut Config| cfg.api.schema.strict = false;
    let s = start_admin_session(Some(&configure));

    let before = s.must_do(200, "GET", "/stats", "", &[]).object();
    for _ in 0..5 {
        s.must_do(200, "GET", "/api/nothing", "", &[]);
    }
    s.must_do(204, "PUT", "/kv/k", "v", &[]);
    let after = s.must_do(200, "GET", "/stats", "", &[]).object();

    let field = |m: &elysiandb::gocompat::json::Map, name: &str| -> String {
        match m.get(name) {
            Some(Value::String(v)) => v.clone(),
            other => panic!("{name} is not a string: {other:?}"),
        }
    };
    let b = scan_int(&field(&before, "total_requests"));
    let a = scan_int(&field(&after, "total_requests"));
    if a - b != 7 {
        panic!(
            "expected 7 more requests (5 lists, 1 KV write, 1 stats), got {}",
            a - b
        );
    }

    if after.get("keys_count") != Some(&vs("1")) {
        panic!(
            "keys_count counts the KV keys only, got {:?}",
            after.get("keys_count")
        );
    }
}

#[test]
fn large_lists_are_streamed_byte_for_byte() {
    let s = start_admin_session(Some(&without_strict_schema));

    let mut list = Vec::with_capacity(4000);
    for i in 0..4000 {
        list.push(obj(vec![
            ("id", vs(&format!("s{i:04}"))),
            ("text", vs(&format!("<&>{}", "t".repeat(260)))),
            ("n", vn(f64::from(i) / 3.0)),
        ]));
    }
    let body = marshal(&Value::Array(list));
    s.must_do(200, "POST", "/api/stream", body, &[]);

    let resp = s.must_do(200, "GET", "/api/stream", "", &[]);
    if resp.body.len() < 1 << 20 {
        panic!(
            "the response should be larger than the streaming threshold, got {} bytes",
            resp.body.len()
        );
    }

    let decoded = json::unmarshal_value(&resp.body).unwrap_or_else(|e| panic!("{e}"));

    let again = marshal(&decoded);
    if again != resp.body {
        panic!("a streamed list must be byte for byte what json.Marshal gives");
    }

    let items = decoded.as_array().cloned().unwrap_or_default();
    if items.len() != 4000 || items[3999].get("id") != Some(&vs("s3999")) {
        panic!("unexpected streamed list of {} documents", items.len());
    }
}

#[test]
fn concurrent_writers_and_readers_on_one_entity() {
    let s = start_admin_session(Some(&without_strict_schema));
    s.must_do(200, "POST", "/api/par", r#"{"id":"seed","rank":-1}"#, &[]);
    s.must_do(204, "DELETE", "/api/par/seed", "", &[]);

    let shared = &s;
    std::thread::scope(|scope| {
        for g in 0..8 {
            let s = shared;
            scope.spawn(move || {
                let mut batch = Vec::with_capacity(50);
                for i in 0..50 {
                    batch.push(obj(vec![
                        ("id", vs(&format!("g{g}-{i:02}"))),
                        ("rank", vn(f64::from(g * 100 + i))),
                    ]));
                }
                let body = marshal(&Value::Array(batch));
                s.must_do(200, "POST", "/api/par", body, &[]);
            });

            scope.spawn(move || {
                for _ in 0..10 {
                    s.must_do(200, "GET", "/api/par?sort%5Brank%5D=desc&limit=5", "", &[]);
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
        }
    });

    let count = s.must_do(200, "GET", "/api/par/count", "", &[]).object();
    if count.get("count") != Some(&vn(400.0)) {
        panic!("expected 400 documents, got {:?}", count.get("count"));
    }

    let sorted = s
        .must_do(200, "GET", "/api/par?sort%5Brank%5D=asc", "", &[])
        .ids();
    if sorted.len() != 400 || sorted[0] != "g0-00" {
        panic!("the sorted list holds every document, got {}", sorted.len());
    }
}

#[test]
fn hooked_responses_larger_than_the_spool_are_sent_whole() {
    let configure = |cfg: &mut Config| {
        cfg.api.schema.strict = false;
        cfg.api.response.spool_mb = 1;
    };
    let s = start_admin_session(Some(&configure));

    let mut list = Vec::with_capacity(3000);
    for i in 0..3000 {
        list.push(obj(vec![
            ("id", vs(&format!("h{i:04}"))),
            ("text", vs(&"x".repeat(400))),
        ]));
    }
    let body = marshal(&Value::Array(list));
    s.must_do(200, "POST", "/api/spooled", body, &[]);
    s.must_do(200, "POST", "/api/hook/spooled", r#"{"name":"mark","event":"post_read","language":"javascript","enabled":true,"script":"function postRead(ctx){ ctx.entity.seen = true }"}"#, &[]);

    let resp = s.must_do(200, "GET", "/api/spooled", "", &[]);
    if resp.body.len() < 1 << 20 {
        panic!(
            "expected a response larger than the spool, got {} bytes",
            resp.body.len()
        );
    }

    let decoded = json::unmarshal_value(&resp.body)
        .unwrap_or_else(|e| panic!("a spooled response must be complete JSON: {e}"));

    let items = decoded.as_array().cloned().unwrap_or_default();
    if items.len() != 3000 || items[2999].get("seen") != Some(&Value::Bool(true)) {
        panic!("every document goes through the hook, got {}", items.len());
    }

    let again = marshal(&decoded);
    if again != resp.body {
        panic!("a spooled response is byte for byte what json.Marshal gives");
    }
}
