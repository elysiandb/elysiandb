use std::collections::BTreeMap;
use std::time::Duration;

use elysiandb::gocompat::json::{self, Value};

use crate::harness::{map, marshal, start_test_server, vn, vs};

#[derive(Debug)]
struct GetEntry {
    key: String,
    value: Option<String>,
}

type MultiGetEntry = GetEntry;

fn entry_of(v: &Value) -> GetEntry {
    let field = |name: &str| {
        v.as_object().and_then(|m| {
            m.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v)
        })
    };
    GetEntry {
        key: field("key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        value: field("value").and_then(Value::as_str).map(str::to_string),
    }
}

fn to_map(arr: &[MultiGetEntry]) -> BTreeMap<String, Option<String>> {
    let mut m = BTreeMap::new();
    for e in arr {
        m.insert(e.key.clone(), e.value.clone());
    }
    m
}

fn must_body_json(b: &[u8]) -> Value {
    json::unmarshal_value(b)
        .unwrap_or_else(|e| panic!("invalid JSON: {e} (body={:?})", String::from_utf8_lossy(b)))
}

fn must_get_entry(b: &[u8]) -> GetEntry {
    entry_of(&must_body_json(b))
}

fn must_multi_get_entries(b: &[u8]) -> Vec<MultiGetEntry> {
    match must_body_json(b).as_array() {
        Some(items) => items.iter().map(entry_of).collect(),
        None => panic!(
            "invalid JSON: not an array (body={:?})",
            String::from_utf8_lossy(b)
        ),
    }
}

#[test]
fn multiple_put_and_mget_with_wildcard() {
    let client = start_test_server();

    for (k, v) in [("baz", "bat"), ("foo", "bar")] {
        let resp = client.do_("PUT", &format!("http://test/kv/{k}"), v);
        if resp.status != 204 {
            panic!("expected 204, got {}", resp.status);
        }
    }

    let resp = client.do_("GET", "http://test/kv/mget?keys=foo,baz,qux,ba*", "");
    if resp.status != 200 {
        panic!("expected 200, got {}", resp.status);
    }

    let arr = must_multi_get_entries(&resp.body);
    let got = to_map(&arr);

    if got.len() != 3 {
        panic!("expected 3 distinct results, got {} ({arr:?})", got.len());
    }
    if got.get("foo").cloned().flatten().as_deref() != Some("bar") {
        panic!("foo mismatch: {:?}", got.get("foo"));
    }
    if got.get("baz").cloned().flatten().as_deref() != Some("bat") {
        panic!("baz mismatch: {:?}", got.get("baz"));
    }
    match got.get("qux") {
        Some(None) => {}
        v => panic!("qux should be present with nil value, got: {v:?}"),
    }
}

#[test]
fn put_and_get() {
    let client = start_test_server();

    let resp = client.do_("PUT", "http://test/kv/foo", "bar");
    if resp.status != 204 {
        panic!("expected 204, got {}", resp.status);
    }

    let resp = client.do_("GET", "http://test/kv/foo", "");
    if resp.status != 200 {
        panic!("expected 200, got {}", resp.status);
    }

    let e = must_get_entry(&resp.body);
    if e.key != "foo" || e.value.as_deref() != Some("bar") {
        panic!("unexpected getEntry: {e:?}");
    }
}

#[test]
fn put_with_ttl_expires() {
    let client = start_test_server();

    let resp = client.do_("PUT", "http://test/kv/ephemeral?ttl=1", "tmp");
    if resp.status != 204 {
        panic!("expected 204, got {}", resp.status);
    }

    std::thread::sleep(Duration::from_millis(1100));

    let resp = client.do_("GET", "http://test/kv/ephemeral", "");
    if resp.status != 404 {
        panic!("expected 404 after TTL, got {}", resp.status);
    }
    let e = must_get_entry(&resp.body);
    if e.key != "ephemeral" || e.value.is_some() {
        panic!("unexpected 404 body: {e:?}");
    }
}

#[test]
fn get_not_found() {
    let client = start_test_server();

    let resp = client.do_("GET", "http://test/kv/notfound", "");
    if resp.status != 404 {
        panic!("expected 404 for missing key, got {}", resp.status);
    }
    let e = must_get_entry(&resp.body);
    if e.key != "notfound" || e.value.is_some() {
        panic!("unexpected 404 body: {e:?}");
    }
}

#[test]
fn put_and_save() {
    let client = start_test_server();

    let resp = client.do_("PUT", "http://test/kv/foo", "bar");
    if resp.status != 204 {
        panic!("expected 204, got {}", resp.status);
    }

    let resp = client.do_("POST", "http://test/save", "");
    if resp.status != 204 {
        panic!("expected 204, got {}", resp.status);
    }

    let resp = client.do_("GET", "http://test/kv/foo", "");
    if resp.status != 200 {
        panic!("expected 200, got {}", resp.status);
    }
    let e = must_get_entry(&resp.body);
    if e.key != "foo" || e.value.as_deref() != Some("bar") {
        panic!("unexpected getEntry: {e:?}");
    }
}

#[test]
fn put_and_reset() {
    let client = start_test_server();

    let resp = client.do_("PUT", "http://test/kv/foo2", "bar");
    if resp.status != 204 {
        panic!("expected 204, got {}", resp.status);
    }

    let resp = client.do_("POST", "http://test/reset", "");
    if resp.status != 200 {
        panic!("expected 200, got {}", resp.status);
    }

    let resp = client.do_("GET", "http://test/kv/foo2", "");
    if resp.status != 404 {
        panic!("expected 404 for missing key, got {}", resp.status);
    }
    let e = must_get_entry(&resp.body);
    if e.key != "foo2" || e.value.is_some() {
        panic!("unexpected 404 body: {e:?}");
    }
}

#[derive(Debug, Default)]
struct StatsDto {
    keys_count: String,
    expiration_keys_count: String,
    uptime_seconds: String,
    total_requests: String,
    hits: String,
    misses: String,
}

#[test]
fn stats() {
    let client = start_test_server();

    {
        let resp = client.do_("PUT", "http://test/kv/foo", "bar");
        if resp.status != 204 {
            panic!("expected 204, got {}", resp.status);
        }
    }

    {
        let resp = client.do_("GET", "http://test/kv/foo", "");
        if resp.status != 200 {
            panic!("expected 200, got {}", resp.status);
        }
    }

    {
        let resp = client.do_("GET", "http://test/kv/missing", "");
        if resp.status != 404 {
            panic!("expected 404, got {}", resp.status);
        }
    }

    let atoi = |s: &str| -> u64 {
        let mut n: u64 = 0;
        for c in s.bytes() {
            if !c.is_ascii_digit() {
                panic!("expected numeric string, got {s:?}");
            }
            n = n.wrapping_mul(10).wrapping_add(u64::from(c - b'0'));
        }
        n
    };

    let get_stats = || -> StatsDto {
        let resp = client.do_("GET", "http://test/stats", "");
        if resp.status != 200 {
            panic!("expected 200 from /stats, got {}", resp.status);
        }

        let v = must_body_json(&resp.body);
        let field = |name: &str| -> String {
            v.as_object()
                .and_then(|m| {
                    m.iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(name))
                        .map(|(_, v)| v)
                })
                .map(|v| match v.as_str() {
                    Some(s) => s.to_string(),
                    None => panic!("invalid JSON: {name} is not a string ({v:?})"),
                })
                .unwrap_or_default()
        };
        let dto = StatsDto {
            keys_count: field("keys_count"),
            expiration_keys_count: field("expiration_keys_count"),
            uptime_seconds: field("uptime_seconds"),
            total_requests: field("total_requests"),
            hits: field("hits"),
            misses: field("misses"),
        };

        let _ = atoi(&dto.keys_count);
        let _ = atoi(&dto.expiration_keys_count);
        let _ = atoi(&dto.uptime_seconds);
        let _ = atoi(&dto.total_requests);
        let _ = atoi(&dto.hits);
        let _ = atoi(&dto.misses);

        dto
    };

    let s1 = get_stats();

    let kc = atoi(&s1.keys_count);
    if kc != 1 {
        panic!("keys_count = {kc}, want 1");
    }
    let ec = atoi(&s1.expiration_keys_count);
    if ec != 0 {
        panic!("expiration_keys_count = {ec}, want 0");
    }
    let _ = atoi(&s1.uptime_seconds);

    let h1 = atoi(&s1.hits);
    let m1 = atoi(&s1.misses);
    let tr1 = atoi(&s1.total_requests);

    if h1 < 1 {
        panic!("hits = {h1}, want >= 1 (after GET on existing key)");
    }
    if m1 < 1 {
        panic!("misses = {m1}, want >= 1 (after GET on missing key)");
    }
    if tr1 < h1 + m1 {
        panic!("total_requests = {tr1}, want >= hits+misses ({})", h1 + m1);
    }
}

#[test]
fn get_wildcard_matches() {
    let client = start_test_server();

    for (k, v) in [("prod:1", "sku1"), ("user:1", "alice"), ("user:2", "bob")] {
        client.do_("PUT", &format!("http://test/kv/{k}"), v);
    }

    let resp = client.do_("GET", "http://test/kv/user:%2A", "");
    if resp.status != 200 {
        panic!("expected 200, got {}", resp.status);
    }

    let arr = must_multi_get_entries(&resp.body);
    let got = to_map(&arr);

    if got.len() != 2 {
        panic!("expected 2 matches, got {} ({arr:?})", got.len());
    }
    if got.get("user:1").cloned().flatten().as_deref() != Some("alice") {
        panic!("user:1 mismatch: {:?}", got.get("user:1"));
    }
    if got.get("user:2").cloned().flatten().as_deref() != Some("bob") {
        panic!("user:2 mismatch: {:?}", got.get("user:2"));
    }
}

#[test]
fn get_wildcard_no_matches_returns_pattern_with_nil() {
    let client = start_test_server();

    let resp = client.do_("GET", "http://test/kv/unknown:%2A", "");
    if resp.status != 200 {
        panic!("expected 200, got {}", resp.status);
    }

    let arr = must_multi_get_entries(&resp.body);

    if arr.len() != 1 || arr[0].key != "unknown:*" || arr[0].value.is_some() {
        panic!("expected single {{key=pattern,value=nil}}, got: {arr:?}");
    }
}

#[test]
fn delete_wildcard_http() {
    let client = start_test_server();

    let seed = [("prod:1", "sku1"), ("user:1", "alice"), ("user:2", "bob")];
    for (k, v) in seed {
        let resp = client.do_("PUT", &format!("http://test/kv/{k}"), v);
        if resp.status != 204 {
            panic!("PUT {k} expected 204, got {}", resp.status);
        }
    }

    {
        let resp = client.do_("DELETE", "http://test/kv/user:%2A", "");
        if resp.status != 204 {
            panic!("DELETE wildcard expected 204, got {}", resp.status);
        }
    }

    {
        let resp = client.do_("GET", "http://test/kv/user:%2A", "");
        if resp.status != 200 {
            panic!("expected 200, got {}", resp.status);
        }
        let arr = must_multi_get_entries(&resp.body);
        if arr.len() != 1 || arr[0].key != "user:*" || arr[0].value.is_some() {
            panic!("expected single {{key=pattern,value=nil}}, got: {arr:?}");
        }
    }

    {
        let resp = client.do_("GET", "http://test/kv/mget?keys=user:1,user:2,prod:1", "");
        if resp.status != 200 {
            panic!("expected 200, got {}", resp.status);
        }

        let arr = must_multi_get_entries(&resp.body);
        let got = to_map(&arr);

        match got.get("user:1") {
            Some(None) => {}
            v => panic!("user:1 should be nil after delete, got: {v:?}"),
        }
        match got.get("user:2") {
            Some(None) => {}
            v => panic!("user:2 should be nil after delete, got: {v:?}"),
        }
        if got.get("prod:1").cloned().flatten().as_deref() != Some("sku1") {
            panic!(
                "prod:1 should still exist with value sku1, got: {:?}",
                got.get("prod:1")
            );
        }
    }
}

#[test]
fn delete_single_http() {
    let client = start_test_server();

    let resp = client.do_("PUT", "http://test/kv/tmpdel", "gone");
    if resp.status != 204 {
        panic!("expected 204, got {}", resp.status);
    }

    let resp = client.do_("DELETE", "http://test/kv/tmpdel", "");
    if resp.status != 204 {
        panic!("expected 204, got {}", resp.status);
    }

    let resp = client.do_("GET", "http://test/kv/tmpdel", "");
    if resp.status != 404 {
        panic!("expected 404 for deleted key, got {}", resp.status);
    }
    let e = must_get_entry(&resp.body);
    if e.key != "tmpdel" || e.value.is_some() {
        panic!("unexpected 404 body: {e:?}");
    }
}

#[test]
fn create_list_of_entities() {
    let client = start_test_server();

    let entities = Value::Array(vec![
        Value::Object(map(vec![("name", vs("Alice")), ("age", vn(30.0))])),
        Value::Object(map(vec![("name", vs("Bob")), ("age", vn(25.0))])),
    ]);

    let body = marshal(&entities);
    let resp = client.do_("POST", "http://test/api/users", body);
    if resp.status != 200 {
        panic!("expected 200, got {}", resp.status);
    }

    let created = must_body_json(&resp.body);
    let created = created.as_array().cloned().unwrap_or_default();
    if created.len() != 2 {
        panic!("expected 2 entities, got {}", created.len());
    }
    for e in &created {
        if e.get("id").and_then(Value::as_str).is_none() {
            panic!("expected id field in {e:?}");
        }
    }
}

#[test]
fn update_list_of_entities() {
    let client = start_test_server();

    let entities = Value::Array(vec![
        Value::Object(map(vec![
            ("id", vs("u1")),
            ("name", vs("Alice")),
            ("age", vn(30.0)),
        ])),
        Value::Object(map(vec![
            ("id", vs("u2")),
            ("name", vs("Bob")),
            ("age", vn(25.0)),
        ])),
    ]);
    let create_body = marshal(&entities);
    let create_resp = client.do_("POST", "http://test/api/users", create_body);
    if create_resp.status != 200 {
        panic!("expected 200, got {}", create_resp.status);
    }

    let updates = Value::Array(vec![
        Value::Object(map(vec![("id", vs("u1")), ("age", vn(35.0))])),
        Value::Object(map(vec![("id", vs("u2")), ("name", vs("Bobby"))])),
    ]);
    let update_body = marshal(&updates);
    let update_resp = client.do_("PUT", "http://test/api/users", update_body);
    if update_resp.status != 200 {
        panic!("expected 200, got {}", update_resp.status);
    }

    let updated = must_body_json(&update_resp.body);
    let updated = updated.as_array().cloned().unwrap_or_default();
    if updated.len() != 2 {
        panic!("expected 2 updated entities, got {}", updated.len());
    }

    for e in &updated {
        let id = e.get("id").and_then(Value::as_str);
        if id == Some("u1") {
            match e.get("age").and_then(Value::as_f64) {
                Some(35.0) => {}
                Some(_) => panic!("expected age=35 for u1, got {e:?}"),
                None => panic!("age is not a number in {e:?}"),
            }
        }
        if id == Some("u2") {
            match e.get("name").and_then(Value::as_str) {
                Some("Bobby") => {}
                Some(_) => panic!("expected name=Bobby for u2, got {e:?}"),
                None => panic!("name is not a string in {e:?}"),
            }
        }
    }
}
