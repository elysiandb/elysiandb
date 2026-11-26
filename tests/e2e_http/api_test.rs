use std::time::{Duration, Instant};

use elysiandb::api as api_storage;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{self, Map, Value};

use crate::harness::{
    ApiServer, Configure, as_object, map, marshal, obj, start_admin_session, start_api_server, vn,
    vs,
};

struct VersionGuard(Option<String>);

impl Drop for VersionGuard {
    fn drop(&mut self) {
        globals::set_version(self.0.take());
    }
}

fn str_of(v: Option<&Value>) -> &str {
    v.and_then(Value::as_str).unwrap_or("")
}

#[test]
fn health_does_not_require_credentials() {
    for mode in ["user", "token", "basic"] {
        let configure = |cfg: &mut Config| {
            cfg.security.authentication.mode = mode.to_string();
        };
        let s = start_api_server(Some(&configure));

        let resp = s.must_do(200, "GET", "/health", "", &[]);
        let got = resp.header("X-Elysian-Version");
        if got != globals::version() {
            panic!(
                "{mode}: expected X-Elysian-Version {:?}, got {got:?}",
                globals::version()
            );
        }

        s.must_do(401, "GET", "/api/book", "", &[]);
    }
}

#[test]
fn version_header_reports_build_version() {
    let _restore = VersionGuard(None);
    globals::set_version(Some("1.2.3-test".to_string()));

    let s = start_api_server(None);

    let got = s
        .must_do(200, "GET", "/health", "", &[])
        .header("X-Elysian-Version");
    if got != "1.2.3-test" {
        panic!("expected the injected version, got {got:?}");
    }
}

#[test]
fn hook_update_keeps_omitted_fields() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/hk", r#"{"id":"v1","n":1}"#, &[]);
    let created = s.must_do(200, "POST", "/api/hook/hk", r#"{"name":"tag","event":"post_read","language":"javascript","priority":5,"enabled":true,"script":"function postRead(ctx) { ctx.entity.tagged = true }"}"#, &[]).object();
    let hook_id = str_of(created.get("id")).to_string();

    let got = s.must_do(200, "GET", "/api/hk/v1", "", &[]).object();
    if got.get("tagged") != Some(&Value::Bool(true)) {
        panic!(
            "expected the hook to run before the update, got {:?}",
            got.get("tagged")
        );
    }

    let updated = s
        .must_do(
            200,
            "PUT",
            &format!("/api/hook/id/{hook_id}"),
            r#"{"script":"function postRead(ctx) { ctx.entity.tagged = 2 }"}"#,
            &[],
        )
        .object();
    let want = [
        ("enabled", Value::Bool(true)),
        ("entity", vs("hk")),
        ("event", vs("post_read")),
        ("language", vs("javascript")),
        ("name", vs("tag")),
        ("priority", vn(5.0)),
    ];
    for (field, value) in &want {
        if updated.get(*field) != Some(value) {
            panic!(
                "expected {field}={value:?} after a partial update, got {:?} ({updated:?})",
                updated.get(*field)
            );
        }
    }

    let hooks = s.must_do(200, "GET", "/api/hook/hk", "", &[]).list();
    if hooks.len() != 1 {
        panic!("expected the hook to stay attached to its entity, got {hooks:?}");
    }

    let got = s.must_do(200, "GET", "/api/hk/v1", "", &[]).object();
    if got.get("tagged") != Some(&vn(2.0)) {
        panic!(
            "expected the updated script to run, got {:?}",
            got.get("tagged")
        );
    }

    s.must_do(
        404,
        "PUT",
        "/api/hook/id/does-not-exist",
        r#"{"name":"x"}"#,
        &[],
    );
    s.must_do(
        400,
        "PUT",
        &format!("/api/hook/id/{hook_id}"),
        r#"{"name":""}"#,
        &[],
    );

    let got = s
        .must_do(200, "GET", &format!("/api/hook/id/{hook_id}"), "", &[])
        .object();
    if got.get("name") != Some(&vs("tag")) {
        panic!(
            "a rejected update must not change the hook, got name {:?}",
            got.get("name")
        );
    }
}

#[test]
fn hook_routes_with_authentication_disabled() {
    let configure = |cfg: &mut Config| cfg.security.authentication.enabled = false;
    let s = start_api_server(Some(&configure));

    s.must_do(200, "GET", "/api/movie", "", &[]);
    s.must_do(200, "GET", "/api/hook/movie", "", &[]);
    let created = s.must_do(200, "POST", "/api/hook/movie", r#"{"name":"h","event":"post_read","language":"javascript","script":"function postRead(ctx){ ctx.entity.seen = true }","enabled":true}"#, &[]).object();
    let id = str_of(created.get("id")).to_string();
    s.must_do(200, "GET", &format!("/api/hook/id/{id}"), "", &[]);
    s.must_do(
        200,
        "PUT",
        &format!("/api/hook/id/{id}"),
        r#"{"priority":3}"#,
        &[],
    );
}

#[test]
fn hook_routes_in_basic_mode() {
    let configure = |cfg: &mut Config| cfg.security.authentication.mode = "basic".to_string();
    let s = start_api_server(Some(&configure));

    s.must_do(401, "GET", "/api/hook/movie", "", &[]);

    let auth = format!("Basic {}", STD_ENCODING.encode_to_string(b"admin:admin"));
    s.must_do(200, "GET", "/api/hook/movie", "", &["Authorization", &auth]);
    s.must_do(200, "POST", "/api/hook/movie", r#"{"name":"h","event":"post_read","language":"javascript","script":"function postRead(ctx){}","enabled":true}"#, &["Authorization", &auth]);

    let wrong = format!("Basic {}", STD_ENCODING.encode_to_string(b"admin:wrong"));
    s.must_do(
        401,
        "GET",
        "/api/hook/movie",
        "",
        &["Authorization", &wrong],
    );
}

#[test]
fn strict_manual_schema_partial_update() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/film/create",
        r#"{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"}}}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/film",
        r#"{"id":"f1","title":"C","year":1983}"#,
        &[],
    );

    s.must_do(200, "PUT", "/api/film/f1", r#"{"year":1984}"#, &[]);
    let film = s.must_do(200, "GET", "/api/film/f1", "", &[]).object();
    if film.get("title") != Some(&vs("C")) || film.get("year") != Some(&vn(1984.0)) {
        panic!("expected a partial update to keep the title, got {film:?}");
    }

    s.must_do(
        200,
        "PUT",
        "/api/film/f1",
        r#"{"title":"C2","year":null}"#,
        &[],
    );
    let film = s.must_do(200, "GET", "/api/film/f1", "", &[]).object();
    if film.get("year") != Some(&Value::Null) || film.get("title") != Some(&vs("C2")) {
        panic!("expected year to be unset with null, got {film:?}");
    }

    let resp = s.must_do(400, "PUT", "/api/film/f1", r#"{"title":null}"#, &[]);
    if !resp.text().contains("required field cannot be null") {
        panic!(
            "expected a null required field to be rejected, got {}",
            resp.text()
        );
    }

    let resp = s.must_do(400, "PUT", "/api/film/f1", r#"{"rating":5}"#, &[]);
    if !resp.text().contains("field not allowed by strict schema") {
        panic!(
            "expected an unknown field to be rejected, got {}",
            resp.text()
        );
    }

    let resp = s.must_do(400, "PUT", "/api/film/f1", r#"{"year":"1999"}"#, &[]);
    if !resp.text().contains("expected type number") {
        panic!("expected a wrong type to be rejected, got {}", resp.text());
    }

    s.must_do(200, "PUT", "/api/film", r#"[{"id":"f1","year":2001}]"#, &[]);
    let resp = s.must_do(400, "PUT", "/api/film", r#"[{"id":"f1","year":"x"}]"#, &[]);
    if !resp.text().contains("expected type number") {
        panic!(
            "expected an invalid batch update to be rejected, got {}",
            resp.text()
        );
    }
    let film = s.must_do(200, "GET", "/api/film/f1", "", &[]).object();
    if film.get("year") != Some(&vn(2001.0)) || film.get("title") != Some(&vs("C2")) {
        panic!("expected a partial batch update to keep the title, got {film:?}");
    }

    s.must_do(400, "POST", "/api/film", r#"{"id":"f2","year":1990}"#, &[]);
}

#[test]
fn strict_manual_schema_update_ignores_untouched_stored_fields() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/doc/create",
        r#"{"fields":{"title":{"type":"string","required":true},"legacy":{"type":"string"}}}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/doc",
        r#"{"id":"d1","title":"T","legacy":"old"}"#,
        &[],
    );
    s.must_do(
        200,
        "PUT",
        "/api/doc/schema",
        r#"{"fields":{"title":{"type":"string","required":true}}}"#,
        &[],
    );

    s.must_do(200, "PUT", "/api/doc/d1", r#"{"title":"T2"}"#, &[]);
    s.must_do(400, "PUT", "/api/doc/d1", r#"{"legacy":"new"}"#, &[]);
}

#[test]
fn inferred_schema_is_extended_not_replaced() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/inf", r#"{"id":"i1","a":"x","b":1}"#, &[]);
    s.must_do(200, "POST", "/api/inf", r#"{"id":"i2","c":true}"#, &[]);

    let fields = as_object(
        s.must_do(200, "GET", "/api/inf/schema", "", &[])
            .object()
            .get("fields"),
    );
    for (name, typ) in [("a", "string"), ("b", "number"), ("c", "boolean")] {
        let ok = matches!(fields.get(name), Some(Value::Object(field)) if field.get("type") == Some(&vs(typ)));
        if !ok {
            panic!("expected field {name} of type {typ} in the inferred schema, got {fields:?}");
        }
    }

    s.must_do(400, "POST", "/api/inf", r#"{"id":"i3","a":5}"#, &[]);
    s.must_do(200, "POST", "/api/inf", r#"{"id":"i4","d":[1,2]}"#, &[]);
}

#[test]
fn inferred_schema_accepts_null_and_later_types() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/nul",
        r#"{"id":"n1","x":null,"y":"s"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/nul",
        r#"{"id":"n2","x":"a","y":null}"#,
        &[],
    );
    s.must_do(400, "POST", "/api/nul", r#"{"id":"n3","x":5}"#, &[]);
    s.must_do(400, "POST", "/api/nul", r#"{"id":"n4","y":5}"#, &[]);
}

#[test]
fn shorthand_entity_type_creation() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/bk/create",
        r#"{"fields":{"title":"string","pages":"number"}}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/bk",
        r#"{"title":"Dune","pages":700}"#,
        &[],
    );
    s.must_do(400, "POST", "/api/bk", r#"{"title":5}"#, &[]);
    s.must_do(
        400,
        "POST",
        "/api/bk",
        r#"{"title":"Dune","author":"x"}"#,
        &[],
    );
}

pub fn begin_transaction(s: &ApiServer) -> String {
    str_of(
        s.must_do(200, "POST", "/api/tx/begin", "", &[])
            .object()
            .get("transaction_id"),
    )
    .to_string()
}

#[test]
fn failed_commit_applies_no_operation() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/acct", r#"{"id":"B","balance":100}"#, &[]);
    let tx = begin_transaction(&s);
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/acct"),
        r#"{"id":"A","balance":50}"#,
        &[],
    );
    s.must_do(
        200,
        "DELETE",
        &format!("/api/tx/{tx}/entity/acct/B"),
        "",
        &[],
    );
    s.must_do(
        200,
        "PUT",
        &format!("/api/tx/{tx}/entity/acct/B"),
        r#"{"balance":0}"#,
        &[],
    );

    let resp = s.must_do(400, "POST", &format!("/api/tx/{tx}/commit"), "", &[]);
    if !resp.text().contains("update failed") {
        panic!(
            "expected the commit to report the failing update, got {}",
            resp.text()
        );
    }

    s.must_do(404, "GET", "/api/acct/A", "", &[]);
    let got = s.must_do(200, "GET", "/api/acct/B", "", &[]).object();
    if got.get("balance") != Some(&vn(100.0)) {
        panic!(
            "expected B to be left untouched, got balance {:?}",
            got.get("balance")
        );
    }
}

#[test]
fn commit_fails_when_target_is_deleted_before_commit() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/acct", r#"{"id":"B","balance":100}"#, &[]);
    let tx = begin_transaction(&s);
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/acct"),
        r#"{"id":"A","balance":50}"#,
        &[],
    );
    s.must_do(
        200,
        "PUT",
        &format!("/api/tx/{tx}/entity/acct/B"),
        r#"{"balance":0}"#,
        &[],
    );
    s.must_do(204, "DELETE", "/api/acct/B", "", &[]);

    s.must_do(400, "POST", &format!("/api/tx/{tx}/commit"), "", &[]);
    s.must_do(404, "GET", "/api/acct/A", "", &[]);
}

#[test]
fn commit_validation_failure_applies_no_operation() {
    let s = start_admin_session(None);

    let tx = begin_transaction(&s);
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/val"),
        r#"{"id":"x1","a":"s"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/val"),
        r#"{"id":"x2","a":5}"#,
        &[],
    );

    let resp = s.must_do(400, "POST", &format!("/api/tx/{tx}/commit"), "", &[]);
    if !resp.text().contains("validation error") {
        panic!("expected a validation error, got {}", resp.text());
    }

    s.must_do(404, "GET", "/api/val/x1", "", &[]);
}

#[test]
fn commit_applies_operations_that_depend_on_each_other() {
    let s = start_admin_session(None);

    let tx = begin_transaction(&s);
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/post"),
        r#"{"id":"p1","title":"T","author":{"@entity":"writer","id":"w1","name":"Old"}}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/post"),
        r#"{"id":"p2","title":"U"}"#,
        &[],
    );
    s.must_do(200, "POST", &format!("/api/tx/{tx}/commit"), "", &[]);

    let tx = begin_transaction(&s);
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/post"),
        r#"{"id":"p3","title":"V","author":{"@entity":"writer","id":"w2","name":"New"}}"#,
        &[],
    );
    s.must_do(
        200,
        "DELETE",
        &format!("/api/tx/{tx}/entity/post/p2"),
        "",
        &[],
    );
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/post"),
        r#"{"id":"p2","title":"U2"}"#,
        &[],
    );
    s.must_do(200, "POST", &format!("/api/tx/{tx}/commit"), "", &[]);

    let got = s.must_do(200, "GET", "/api/writer/w2", "", &[]).object();
    if got.get("name") != Some(&vs("New")) {
        panic!(
            "expected the nested writer to be created, got {:?}",
            got.get("name")
        );
    }

    let got = s.must_do(200, "GET", "/api/post/p2", "", &[]).object();
    if got.get("title") != Some(&vs("U2")) {
        panic!(
            "expected p2 to be rewritten after its delete, got {:?}",
            got.get("title")
        );
    }
}

#[test]
fn query_cache_key_includes_count_only_and_fields() {
    let s = start_admin_session(None);

    for i in 1..=3 {
        s.must_do(
            200,
            "POST",
            "/api/book",
            format!(r#"{{"id":"b{i}","title":"Book {i}","pages":{}}}"#, i * 100),
            &[],
        );
    }

    s.must_do(200, "POST", "/api/query", r#"{"entity":"book"}"#, &[]);

    let count = s.must_do(
        200,
        "POST",
        "/api/query",
        r#"{"entity":"book","countOnly":true}"#,
        &[],
    );
    if count.text() != r#"{"count":3}"# || count.header("X-Elysian-Cache") != "MISS" {
        panic!(
            "expected a fresh count, got {} ({})",
            count.text(),
            count.header("X-Elysian-Cache")
        );
    }

    let projected = s
        .must_do(
            200,
            "POST",
            "/api/query",
            r#"{"entity":"book","fields":"title"}"#,
            &[],
        )
        .list();
    for item in &projected {
        if item.contains_key("pages") || item.len() != 1 {
            panic!("expected only the projected field, got {item:?}");
        }
    }

    s.must_do(
        200,
        "POST",
        "/api/query",
        r#"{"entity":"book","limit":50,"countOnly":true}"#,
        &[],
    );
    let list = s
        .must_do(
            200,
            "POST",
            "/api/query",
            r#"{"entity":"book","limit":50}"#,
            &[],
        )
        .list();
    if list.len() != 3 {
        panic!("expected the document list, got {list:?}");
    }

    let hit = s.must_do(
        200,
        "POST",
        "/api/query",
        r#"{"entity":"book","countOnly":true}"#,
        &[],
    );
    if hit.header("X-Elysian-Cache") != "HIT" || hit.text() != r#"{"count":3}"# {
        panic!(
            "expected the count to be served from its own cache entry, got {} ({})",
            hit.text(),
            hit.header("X-Elysian-Cache")
        );
    }
}

#[test]
fn search_matches_before_pagination_and_combines_with_filters() {
    let s = start_admin_session(None);

    for i in 1..=4 {
        s.must_do(
            200,
            "POST",
            "/api/note",
            format!(r#"{{"id":"n{i}","title":"Note {i}","kind":"k{}"}}"#, i % 2),
            &[],
        );
    }
    s.must_do(
        200,
        "POST",
        "/api/note",
        r#"{"id":"n5","title":"Zebra crossing","kind":"k1"}"#,
        &[],
    );

    for path in [
        "/api/note?search=*Zebra*",
        "/api/note?search=*Zebra*&limit=2",
        "/api/note?search=Zebra&limit=2",
        "/api/note?search=zebra",
    ] {
        let ids = s.must_do(200, "GET", path, "", &[]).ids();
        if ids != ["n5"] {
            panic!("GET {path}: expected [n5], got {ids:?}");
        }
    }

    let ids = s
        .must_do(
            200,
            "GET",
            "/api/note?search=Note&filter%5Bkind%5D%5Beq%5D=k0",
            "",
            &[],
        )
        .ids();
    if sorted_copy(&ids) != Some(vec!["n2".to_string(), "n4".to_string()]) {
        panic!("expected search and filter to combine, got {ids:?}");
    }

    let ids = s
        .must_do(
            200,
            "GET",
            "/api/note?search=Note&limit=2&offset=3",
            "",
            &[],
        )
        .ids();
    if ids.len() != 1 {
        panic!("expected pagination over the 4 matches, got {ids:?}");
    }

    let body = s
        .must_do(200, "GET", "/api/note?search=Note&countOnly=true", "", &[])
        .text();
    if body != r#"{"count":4}"# {
        panic!("expected 4 matches, got {body}");
    }
}

#[test]
fn search_ignores_owner_markers_and_ids() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/movie",
        r#"{"id":"m1","title":"Alien"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/director",
        r#"{"id":"ridley-scott","name":"Ridley Scott"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/film",
        r#"{"id":"f1","title":"Blade Runner","directors":[{"@entity":"director","id":"ridley-scott"}]}"#,
        &[],
    );

    for path in [
        "/api/movie?search=admin",
        "/api/film?search=scott",
        "/api/film?search=director",
        "/api/movie?search=m1",
        "/api/film?search=admin&includes=directors",
    ] {
        let ids = s.must_do(200, "GET", path, "", &[]).ids();
        if !ids.is_empty() {
            panic!("GET {path}: expected no match, got {ids:?}");
        }
    }

    for (path, want) in [
        ("/api/film?search=runner", "f1"),
        ("/api/film?search=scott&includes=directors", "f1"),
        ("/api/movie?search=ali", "m1"),
    ] {
        let ids = s.must_do(200, "GET", path, "", &[]).ids();
        if ids != [want] {
            panic!("GET {path}: expected [{want}], got {ids:?}");
        }
    }
}

#[test]
fn update_on_missing_document_is_not_found() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/book/create", r#"{"fields":{"title":{"type":"string","required":true},"year":{"type":"number","required":false}}}"#, &[]);
    s.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"id":"b1","title":"Dune","year":1965}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/movie",
        r#"{"id":"m1","title":"Alien"}"#,
        &[],
    );

    for (path, body) in [
        ("/api/book/nope", r#"{"year":2000}"#),
        ("/api/movie/nope", r#"{"title":"x"}"#),
    ] {
        let resp = s.must_do(404, "PUT", path, body, &[]);
        if resp.text() != r#"{"error":"entity not found"}"# {
            panic!(
                "PUT {path}: expected an entity not found error, got {}",
                resp.text()
            );
        }
    }

    let resp = s
        .must_do(
            404,
            "PUT",
            "/api/movie",
            r#"[{"id":"m1","title":"Alien (1979)"},{"id":"nope","title":"x"}]"#,
            &[],
        )
        .object();
    if resp.get("ids") != Some(&Value::Array(vec![vs("nope")])) {
        panic!("expected the missing ids to be listed, got {resp:?}");
    }

    let got = s.must_do(200, "GET", "/api/movie/m1", "", &[]).object();
    if got.get("title") != Some(&vs("Alien")) {
        panic!(
            "a batch with a missing id must not apply any update, got title {:?}",
            got.get("title")
        );
    }

    let body = s.must_do(200, "GET", "/api/movie/count", "", &[]).text();
    if body != r#"{"count":1}"# {
        panic!("expected no document to be created, got {body}");
    }

    s.must_do(200, "PUT", "/api/book/b1", r#"{"year":2000}"#, &[]);
    s.must_do(
        200,
        "PUT",
        "/api/movie",
        r#"[{"id":"m1","title":"Alien (1979)"}]"#,
        &[],
    );
}

#[test]
fn query_multi_field_sorts_are_ordered_and_deterministic() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/flm/create",
        r#"{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"}}}"#,
        &[],
    );
    s.must_do(200, "POST", "/api/flm", r#"[{"id":"f1","title":"C","year":1983},{"id":"f2","title":"B","year":1982},{"id":"f3","title":"A","year":1979},{"id":"f4","title":"A","year":1983}]"#, &[]);

    for i in 0..20 {
        let ids = s
            .must_do(
                200,
                "POST",
                "/api/query",
                format!(
                    r#"{{"entity":"flm","sorts":{{"year":"desc","title":"asc"}},"limit":{}}}"#,
                    10 + i
                ),
                &[],
            )
            .ids();
        if ids != ["f4", "f1", "f2", "f3"] {
            panic!("year desc then title asc: expected [f4 f1 f2 f3], got {ids:?}");
        }

        let ids = s
            .must_do(
                200,
                "POST",
                "/api/query",
                format!(
                    r#"{{"entity":"flm","sorts":{{"title":"asc","year":"desc"}},"limit":{}}}"#,
                    10 + i
                ),
                &[],
            )
            .ids();
        if ids != ["f4", "f3", "f2", "f1"] {
            panic!("title asc then year desc: expected [f4 f3 f2 f1], got {ids:?}");
        }
    }

    let ids = s
        .must_do(
            200,
            "POST",
            "/api/query",
            r#"{"entity":"flm","sorts":{"year":"desc","title":"asc"},"offset":1,"limit":2}"#,
            &[],
        )
        .ids();
    if ids != ["f1", "f2"] {
        panic!("expected pagination after sorting, got {ids:?}");
    }

    let ids = s
        .must_do(
            200,
            "POST",
            "/api/query",
            r#"{"entity":"flm","sorts":{"year":"asc"}}"#,
            &[],
        )
        .ids();
    if ids[0] != "f3" || ids[3] != "f1" && ids[3] != "f4" {
        panic!("expected a single sort to keep working, got {ids:?}");
    }
}

#[test]
fn pre_read_virtual_field_filters_apply_before_pagination() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/num/create",
        r#"{"fields":{"n":{"type":"number","required":true}}}"#,
        &[],
    );
    for i in 1..=6 {
        s.must_do(
            200,
            "POST",
            "/api/num",
            format!(r#"{{"id":"v{i}","n":{i}}}"#),
            &[],
        );
    }
    s.must_do(200, "POST", "/api/hook/num", r#"{"name":"even","event":"pre_read","language":"javascript","priority":10,"enabled":true,"script":"function preRead(ctx) { ctx.entity.even = (ctx.entity.n % 2 === 0) }"}"#, &[]);

    let cases: [(&str, &[&str]); 5] = [
        (
            "/api/num?filter%5Beven%5D%5Beq%5D=true&filter%5Bn%5D%5Bgt%5D=2&sort%5Bn%5D=asc",
            &["v4", "v6"],
        ),
        (
            "/api/num?filter%5Beven%5D%5Beq%5D=true&limit=1&offset=1&sort%5Bn%5D=asc",
            &["v4"],
        ),
        (
            "/api/num?filter%5Beven%5D%5Beq%5D=true&limit=3&sort%5Bn%5D=asc",
            &["v2", "v4", "v6"],
        ),
        (
            "/api/num?filter%5Beven%5D%5Beq%5D=true&sort%5Bn%5D=asc",
            &["v2", "v4", "v6"],
        ),
        ("/api/num?limit=2&offset=1&sort%5Bn%5D=asc", &["v2", "v3"]),
    ];
    for (path, want) in cases {
        let ids = s.must_do(200, "GET", path, "", &[]).ids();
        if ids != want {
            panic!("GET {path}: expected {want:?}, got {ids:?}");
        }
    }

    let page = s
        .must_do(
            200,
            "GET",
            "/api/num?limit=2&offset=1&sort%5Bn%5D=asc",
            "",
            &[],
        )
        .list();
    if page[0].get("even") != Some(&Value::Bool(true))
        || page[1].get("even") != Some(&Value::Bool(false))
    {
        panic!("expected the pre_read hook to run on a page without filter, got {page:?}");
    }

    let queries: [(&str, &[&str]); 5] = [
        (
            r#"{"entity":"num","filters":{"and":[{"even":{"eq":"true"}},{"n":{"gte":4}}]},"sorts":{"n":"desc"}}"#,
            &["v6", "v4"],
        ),
        (
            r#"{"entity":"num","filters":{"even":{"eq":"true"}},"sorts":{"n":"asc"},"limit":3}"#,
            &["v2", "v4", "v6"],
        ),
        (
            r#"{"entity":"num","filters":{"even":{"eq":"true"}},"sorts":{"n":"asc"},"offset":2}"#,
            &["v6"],
        ),
        (
            r#"{"entity":"num","filters":{"even":{"eq":true}},"sorts":{"n":"asc"},"limit":2}"#,
            &["v2", "v4"],
        ),
        (
            r#"{"entity":"num","filters":{"n":{"lte":2}},"sorts":{"n":"asc"}}"#,
            &["v1", "v2"],
        ),
    ];
    for (body, want) in queries {
        let ids = s.must_do(200, "POST", "/api/query", body, &[]).ids();
        if ids != want {
            panic!("POST /api/query {body}: expected {want:?}, got {ids:?}");
        }
    }

    let body = s
        .must_do(
            200,
            "POST",
            "/api/query",
            r#"{"entity":"num","filters":{"even":{"eq":"true"}},"countOnly":true}"#,
            &[],
        )
        .text();
    if body != r#"{"count":3}"# {
        panic!("expected the count to include virtual field filters, got {body}");
    }
}

#[test]
fn query_filter_values_and_errors() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/film", r#"[{"id":"f1","title":"C","year":1983,"color":true},{"id":"f2","title":"B","year":1982.5,"color":false}]"#, &[]);

    let cases = [
        (
            r#"{"entity":"film","filters":{"color":{"eq":false}}}"#,
            "f2",
        ),
        (
            r#"{"entity":"film","filters":{"year":{"eq":"1983"}}}"#,
            "f1",
        ),
        (
            r#"{"entity":"film","filters":{"year":{"eq":1982.5}}}"#,
            "f2",
        ),
        (r#"{"entity":"film","filters":{"year":{"eq":1983}}}"#, "f1"),
    ];
    for (body, want) in cases {
        let ids = s.must_do(200, "POST", "/api/query", body, &[]).ids();
        if ids != [want] {
            panic!("POST /api/query {body}: expected [{want}], got {ids:?}");
        }
    }

    for body in [
        r#"{"entity":"film","filters":{"year":{"eq":null}}}"#,
        r#"{"entity":"film","filters":{"year":{"eq":[1]}}}"#,
        r#"{"entity":"film","filters":{"or":{}}}"#,
        "{bad",
    ] {
        let resp = s.must_do(400, "POST", "/api/query", body, &[]);
        if str_of(resp.object().get("error")).is_empty() {
            panic!(
                "POST /api/query {body}: expected an error message, got {}",
                resp.text()
            );
        }
    }
}

#[test]
fn string_filter_operators() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/tg", r#"[{"id":"t1","tags":["Smith, John","x"],"label":"banana","day":"2024-05-01"},{"id":"t2","tags":["a"],"label":"apple","day":"2024-01-01"},{"id":"t3","tags":["b"],"label":"cherry","day":"2023-01-01"}]"#, &[]);

    let cases: [(&str, Option<&[&str]>); 15] = [
        ("filter%5Bday%5D%5Bcontains%5D=-05-", Some(&["t1"])),
        (
            "filter%5Bday%5D%5Bgte%5D=2024-01-01&filter%5Bday%5D%5Blt%5D=2024-05-01",
            Some(&["t2"]),
        ),
        ("filter%5Bday%5D%5Blt%5D=2024-02-01", Some(&["t2", "t3"])),
        ("filter%5Blabel%5D%5Bany%5D=apple", None),
        ("filter%5Blabel%5D%5Bcontains%5D=AN", Some(&["t1"])),
        ("filter%5Blabel%5D%5Bcontains%5D=an", Some(&["t1"])),
        ("filter%5Blabel%5D%5Bgt%5D=zzz", None),
        ("filter%5Blabel%5D%5Bgte%5D=banana", Some(&["t1", "t3"])),
        ("filter%5Blabel%5D%5Blt%5D=b", Some(&["t2"])),
        (
            "filter%5Blabel%5D%5Bnot_contains%5D=an",
            Some(&["t2", "t3"]),
        ),
        ("filter%5Btags%5D%5Bany%5D=a,b", Some(&["t2", "t3"])),
        (
            "filter%5Btags%5D%5Bcontains%5D=Smith,%20John",
            Some(&["t1"]),
        ),
        ("filter%5Btags%5D%5Bcontains%5D=a", Some(&["t2"])),
        ("filter%5Btags%5D%5Bcontains%5D=a,zzz", None),
        ("filter%5Btags%5D%5Bnot_contains%5D=a", Some(&["t1", "t3"])),
    ];
    for (query, want) in cases {
        let ids = s
            .must_do(200, "GET", &format!("/api/tg?{query}"), "", &[])
            .ids();
        let want = want.map(|w| w.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        if sorted_copy(&ids) != want {
            panic!("GET /api/tg?{query}: expected {want:?}, got {ids:?}");
        }
    }

    let ids = s
        .must_do(
            200,
            "POST",
            "/api/query",
            r#"{"entity":"tg","filters":{"label":{"lt":"b"}}}"#,
            &[],
        )
        .ids();
    if ids != ["t2"] {
        panic!("expected the Query API to compare strings too, got {ids:?}");
    }
}

#[test]
fn cors_allow_list() {
    let configure = |cfg: &mut Config| {
        cfg.server.http.cors.allowed_origins = vec!["https://app.example".to_string()].into();
    };
    let s = start_admin_session(Some(&configure));

    let resp = s.must_do(
        200,
        "GET",
        "/api/book",
        "",
        &["Origin", "https://evil.example"],
    );
    let got = resp.header("Access-Control-Allow-Origin");
    if !got.is_empty() {
        panic!("expected no CORS header for an unknown origin, got {got:?}");
    }

    let resp = s.must_do(
        204,
        "OPTIONS",
        "/api/book",
        "",
        &["Origin", "https://evil.example"],
    );
    let got = resp.header("Access-Control-Allow-Credentials");
    if !got.is_empty() {
        panic!("expected no credentials header on a preflight from an unknown origin, got {got:?}");
    }

    let resp = s.must_do(
        200,
        "GET",
        "/api/book",
        "",
        &["Origin", "https://app.example"],
    );
    if resp.header("Access-Control-Allow-Origin") != "https://app.example"
        || resp.header("Access-Control-Allow-Credentials") != "true"
    {
        panic!(
            "expected CORS headers for an allowed origin, got {:?}",
            resp.headers
        );
    }
}

#[test]
fn cors_is_disabled_by_default() {
    let s = start_admin_session(None);

    let resp = s.must_do(
        200,
        "GET",
        "/api/book",
        "",
        &["Origin", "https://evil.example"],
    );
    let got = resp.header("Access-Control-Allow-Origin");
    if !got.is_empty() {
        panic!("expected no CORS header without an allow-list, got {got:?}");
    }
}

#[test]
fn session_cookie_secure_flag_follows_the_request() {
    let s = start_api_server(None);
    let body = r#"{"username":"admin","password":"admin"}"#;

    let plain = s.must_do(200, "POST", "/api/security/login", body, &[]);
    if plain
        .header("Set-Cookie")
        .to_ascii_lowercase()
        .contains("secure")
    {
        panic!(
            "expected no Secure flag over plain HTTP, got {:?}",
            plain.header("Set-Cookie")
        );
    }
    if !plain
        .header("Set-Cookie")
        .to_ascii_lowercase()
        .contains("httponly")
    {
        panic!(
            "expected the cookie to stay HttpOnly, got {:?}",
            plain.header("Set-Cookie")
        );
    }

    let proxied = s.must_do(
        200,
        "POST",
        "/api/security/login",
        body,
        &["X-Forwarded-Proto", "https"],
    );
    if !proxied
        .header("Set-Cookie")
        .to_ascii_lowercase()
        .contains("secure")
    {
        panic!(
            "expected the Secure flag behind an HTTPS proxy, got {:?}",
            proxied.header("Set-Cookie")
        );
    }
}

#[test]
fn nested_write_purges_sub_entity_cache() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/author",
        r#"{"id":"a1","name":"Old"}"#,
        &[],
    );
    s.must_do(200, "GET", "/api/author", "", &[]);
    s.must_do(200, "GET", "/api/author/a1", "", &[]);
    s.must_do(
        200,
        "POST",
        "/api/novel",
        r#"{"id":"nv1","title":"T","author":{"@entity":"author","id":"a1","name":"New"}}"#,
        &[],
    );

    let list = s.must_do(200, "GET", "/api/author", "", &[]);
    if list.header("X-Elysian-Cache") != "MISS" || list.list()[0].get("name") != Some(&vs("New")) {
        panic!(
            "expected the author list to be refreshed after a nested write, got {} ({})",
            list.text(),
            list.header("X-Elysian-Cache")
        );
    }

    let got = s.must_do(200, "GET", "/api/author/a1", "", &[]).object();
    if got.get("name") != Some(&vs("New")) {
        panic!(
            "expected the author to be refreshed after a nested write, got {:?}",
            got.get("name")
        );
    }

    s.must_do(200, "GET", "/api/author", "", &[]);
    s.must_do(
        200,
        "PUT",
        "/api/novel/nv1",
        r#"{"author":{"@entity":"author","id":"a1","name":"Newer"}}"#,
        &[],
    );
    let got = s.must_do(200, "GET", "/api/author", "", &[]).list();
    if got[0].get("name") != Some(&vs("Newer")) {
        panic!(
            "expected the author list to be refreshed after a nested update, got {:?}",
            got[0].get("name")
        );
    }

    s.must_do(200, "GET", "/api/author", "", &[]);
    let tx = begin_transaction(&s);
    s.must_do(
        200,
        "POST",
        &format!("/api/tx/{tx}/entity/novel"),
        r#"{"id":"nv2","title":"U","author":{"@entity":"author","id":"a1","name":"Newest"}}"#,
        &[],
    );
    s.must_do(200, "POST", &format!("/api/tx/{tx}/commit"), "", &[]);
    let got = s.must_do(200, "GET", "/api/author", "", &[]).list();
    if got[0].get("name") != Some(&vs("Newest")) {
        panic!(
            "expected the author list to be refreshed after a nested transactional write, got {:?}",
            got[0].get("name")
        );
    }
}

#[test]
fn array_post_is_atomic() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/arr", r#"{"id":"p0","a":"x"}"#, &[]);
    let resp = s.must_do(
        400,
        "POST",
        "/api/arr",
        r#"[{"id":"p1","a":"y"},{"id":"p2","a":5}]"#,
        &[],
    );
    if resp.text() != r#"[[],[{"Field":"a","Message":"expected type string but got number"}]]"# {
        panic!("expected per-item validation errors, got {}", resp.text());
    }

    s.must_do(404, "GET", "/api/arr/p1", "", &[]);
    s.must_do(404, "GET", "/api/arr/p2", "", &[]);

    s.must_do(
        400,
        "POST",
        "/api/arr2",
        r#"[{"id":"q1","b":"y"},{"id":"q2","b":5}]"#,
        &[],
    );
    s.must_do(404, "GET", "/api/arr2/q1", "", &[]);

    s.must_do(
        200,
        "POST",
        "/api/arr",
        r#"[{"id":"p3","a":"y"},{"id":"p4","a":"z","c":1}]"#,
        &[],
    );
    s.must_do(200, "GET", "/api/arr/p4", "", &[]);
}

#[test]
fn core_entity_acl_returns_clear_error() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"ops-pass-1","role":"admin"}"#,
        &[],
    );
    for entity in [
        "_elysiandb_core_user",
        "_elysiandb_core_acl",
        "_elysiandb_core_hook",
    ] {
        let resp = s.must_do(
            400,
            "PUT",
            &format!("/api/acl/ops/{entity}"),
            r#"{"permissions":{"read":true}}"#,
            &[],
        );
        if !resp.text().contains("core entities have no ACL") {
            panic!("expected a clear error for {entity}, got {}", resp.text());
        }

        s.must_do(
            400,
            "PUT",
            &format!("/api/acl/ops/{entity}/default"),
            "",
            &[],
        );
        s.must_do(404, "GET", &format!("/api/acl/ops/{entity}"), "", &[]);
    }

    s.must_do(200, "POST", "/api/book", r#"{"title":"x"}"#, &[]);
    s.must_do(
        200,
        "PUT",
        "/api/acl/ops/book",
        r#"{"permissions":{"read":true}}"#,
        &[],
    );
}

#[test]
fn default_and_last_admin_are_protected() {
    let mut s = start_admin_session(None);

    let resp = s.must_do(403, "DELETE", "/api/security/user/admin", "", &[]);
    if !resp.text().contains("cannot be deleted") {
        panic!("expected a clear error, got {}", resp.text());
    }

    let resp = s.must_do(
        409,
        "PUT",
        "/api/security/user/admin/role",
        r#"{"role":"user"}"#,
        &[],
    );
    if !resp.text().contains("at least one admin") {
        panic!("expected a clear error, got {}", resp.text());
    }

    s.must_do(
        400,
        "PUT",
        "/api/security/user/admin/role",
        r#"{"role":"superuser"}"#,
        &[],
    );
    s.must_do(
        200,
        "PUT",
        "/api/security/user/admin/role",
        r#"{"role":"admin"}"#,
        &[],
    );

    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"ops","password":"ops-pass-1","role":"admin"}"#,
        &[],
    );
    s.must_do(
        200,
        "PUT",
        "/api/security/user/admin/role",
        r#"{"role":"user"}"#,
        &[],
    );
    s.must_do(401, "GET", "/api/security/me", "", &[]);
    s.login("ops", "ops-pass-1");
    s.must_do(
        409,
        "PUT",
        "/api/security/user/ops/role",
        r#"{"role":"user"}"#,
        &[],
    );
    s.must_do(409, "DELETE", "/api/security/user/ops", "", &[]);
    s.must_do(
        200,
        "PUT",
        "/api/security/user/admin/role",
        r#"{"role":"admin"}"#,
        &[],
    );

    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"alice","password":"alice-pass","role":"user"}"#,
        &[],
    );
    s.must_do(200, "DELETE", "/api/security/user/alice", "", &[]);
    s.must_do(200, "DELETE", "/api/security/user/ops", "", &[]);
}

#[test]
fn core_entities_are_not_exposed_by_generic_routes() {
    let disabled = |cfg: &mut Config| cfg.security.authentication.enabled = false;
    let token = |cfg: &mut Config| cfg.security.authentication.mode = "token".to_string();
    let modes: [(&str, Configure<'_>); 3] = [
        ("disabled", Some(&disabled)),
        ("token", Some(&token)),
        ("user", None),
    ];

    for (name, configure) in modes {
        let mut s = start_api_server(configure);
        match name {
            "token" => s.headers.set("Authorization", "Bearer test-token"),
            "user" => s.login("admin", "admin"),
            _ => {}
        }

        let requests = [
            ["GET", "/api/_elysiandb_core_user", ""],
            ["GET", "/api/_elysiandb_core_user/admin", ""],
            ["DELETE", "/api/_elysiandb_core_user/admin", ""],
            [
                "PUT",
                "/api/_elysiandb_core_user/admin",
                r#"{"role":"user"}"#,
            ],
            [
                "POST",
                "/api/_elysiandb_core_user",
                r#"{"id":"x","username":"x","password":"x","role":"admin"}"#,
            ],
            ["GET", "/api/_elysiandb_core_acl", ""],
            ["GET", "/api/_elysiandb_core_hook/count", ""],
            ["GET", "/api/_elysiandb_core_schema/schema", ""],
            ["POST", "/api/query", r#"{"entity":"_elysiandb_core_user"}"#],
        ];

        for r in requests {
            let resp = s.must_do(403, r[0], r[1], r[2], &[]);
            if resp.text().contains("password") {
                panic!(
                    "{name}: {} {} leaked user data: {}",
                    r[0],
                    r[1],
                    resp.text()
                );
            }
        }

        let tx = begin_transaction(&s);
        s.must_do(
            403,
            "POST",
            &format!("/api/tx/{tx}/entity/_elysiandb_core_user"),
            r#"{"id":"x"}"#,
            &[],
        );
        s.must_do(
            403,
            "DELETE",
            &format!("/api/tx/{tx}/entity/_elysiandb_core_user/admin"),
            "",
            &[],
        );

        s.must_do(200, "POST", "/api/book", r#"{"title":"still works"}"#, &[]);
    }
}

#[test]
fn config_redacts_secrets() {
    let configure = |cfg: &mut Config| cfg.security.authentication.mode = "token".to_string();
    let mut s = start_api_server(Some(&configure));
    s.headers.set("Authorization", "Bearer test-token");

    let resp = s.must_do(200, "GET", "/config", "", &[]);
    if resp.text().contains("test-token") {
        panic!("expected secrets to be redacted, got {}", resp.text());
    }

    let security = as_object(resp.object().get("Security"));
    let authentication = as_object(security.get("Authentication"));
    if authentication.get("Token") != Some(&vs("***"))
        || authentication.get("Mode") != Some(&vs("token"))
        || authentication.get("Enabled") != Some(&Value::Bool(true))
    {
        panic!("unexpected authentication block {authentication:?}");
    }

    let engine_block = as_object(resp.object().get("Engine"));
    if engine_block.len() != 1 {
        panic!("expected the engine block to hold only its name, got {engine_block:?}");
    }

    if globals::config().is_none_or(|c| c.security.authentication.token != "test-token") {
        panic!("redaction must not change the running configuration");
    }
}

pub fn sorted_copy(input: &[String]) -> Option<Vec<String>> {
    if input.is_empty() {
        return None;
    }

    let mut out = input.to_vec();
    out.sort();

    Some(out)
}

pub fn without_strict_schema(cfg: &mut Config) {
    cfg.api.schema.strict = false;
}

fn ids_of(v: &[&str]) -> Option<Vec<String>> {
    Some(v.iter().map(|x| x.to_string()).collect())
}

#[test]
fn unvalidated_writes_keep_the_inferred_schema_in_step() {
    let s = start_admin_session(Some(&without_strict_schema));

    s.must_do(200, "POST", "/api/p", r#"{"id":"a","n":"x"}"#, &[]);
    s.must_do(200, "PUT", "/api/p/a", r#"{"n":5}"#, &[]);
    s.must_do(200, "POST", "/api/p", r#"{"id":"b","n":6}"#, &[]);

    s.must_do(
        200,
        "POST",
        "/api/items",
        r#"{"id":"a","status":"active"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/items/migrate",
        r#"[{"set":[{"status":1}]}]"#,
        &[],
    );
    s.must_do(200, "POST", "/api/items", r#"{"id":"b","status":2}"#, &[]);

    s.must_do(200, "POST", "/api/author", r#"{"id":"x","age":30}"#, &[]);
    s.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"id":"b","author":{"@entity":"author","id":"z","age":"thirty"}}"#,
        &[],
    );
    let got = s.must_do(200, "GET", "/api/author/z", "", &[]).object();
    if got.get("age") != Some(&vs("thirty")) {
        panic!(
            "expected the nested author to be stored, got {:?}",
            got.get("age")
        );
    }

    s.must_do(400, "POST", "/api/author", r#"{"id":"w","age":5}"#, &[]);
}

#[test]
fn import_keeps_documents_with_mixed_types() {
    let s = start_admin_session(Some(&without_strict_schema));

    s.must_do(200, "POST", "/api/p", r#"{"id":"a","n":"x"}"#, &[]);
    s.must_do(200, "PUT", "/api/p/a", r#"{"n":5}"#, &[]);
    s.must_do(200, "POST", "/api/p", r#"{"id":"b","n":6}"#, &[]);

    let dump = s.must_do(200, "GET", "/api/export", "", &[]).object();
    let mut wrapped = Map::new();
    wrapped.insert(
        "p".to_string(),
        dump.get("p").cloned().unwrap_or(Value::Null),
    );
    let body = json::marshal(&wrapped).unwrap_or_default();
    s.must_do(200, "POST", "/api/import", body, &[]);
    let ids = sorted_copy(&s.must_do(200, "GET", "/api/p", "", &[]).ids());
    if ids != ids_of(&["a", "b"]) {
        panic!("expected an export/import round trip to keep every document, got {ids:?}");
    }

    s.must_do(
        200,
        "POST",
        "/api/import",
        r#"{"mixed":[{"id":"m1","v":"x"},{"id":"m2","w":1},{"id":"m3","v":2}]}"#,
        &[],
    );
    let ids = sorted_copy(&s.must_do(200, "GET", "/api/mixed", "", &[]).ids());
    if ids != ids_of(&["m1", "m2", "m3"]) {
        panic!("expected the import to keep documents with mixed types, got {ids:?}");
    }

    s.must_do(
        200,
        "POST",
        "/api/strictimp/create",
        r#"{"fields":{"title":{"type":"string","required":true}}}"#,
        &[],
    );
    s.must_do(
        400,
        "POST",
        "/api/import",
        r#"{"strictimp":[{"id":"s1","title":"ok"},{"id":"s2","title":5}]}"#,
        &[],
    );
    let ids = s.must_do(200, "GET", "/api/strictimp", "", &[]).ids();
    if !ids.is_empty() {
        panic!("expected manual schemas to keep validating imports, got {ids:?}");
    }
}

#[test]
fn destroy_resets_the_inferred_schema_only() {
    let s = start_admin_session(Some(&without_strict_schema));

    s.must_do(200, "POST", "/api/sz", r#"{"id":"1","size":"big"}"#, &[]);
    s.must_do(204, "DELETE", "/api/sz", "", &[]);
    s.must_do(200, "POST", "/api/sz", r#"{"id":"2","color":"red"}"#, &[]);
    s.must_do(200, "POST", "/api/sz", r#"{"id":"3","size":4}"#, &[]);

    s.must_do(
        200,
        "POST",
        "/api/man/create",
        r#"{"fields":{"title":{"type":"string","required":true}}}"#,
        &[],
    );
    s.must_do(200, "POST", "/api/man", r#"{"id":"1","title":"x"}"#, &[]);
    s.must_do(204, "DELETE", "/api/man", "", &[]);
    s.must_do(400, "POST", "/api/man", r#"{"id":"2","title":5}"#, &[]);
}

#[test]
fn nested_core_entity_references_are_rejected() {
    let disabled = |cfg: &mut Config| cfg.security.authentication.enabled = false;
    let modes: [(&str, Configure<'_>); 2] = [("disabled", Some(&disabled)), ("user", None)];

    for (name, configure) in modes {
        let mut s = start_api_server(configure);
        if configure.is_none() {
            s.login("admin", "admin");
        }

        let hook_ref = r#"{"@entity":"_elysiandb_core_hook","name":"h","entity":"posts","event":"pre_read","language":"javascript","enabled":true,"script":"function preRead(ctx){ctx.entity.pwn=1}"}"#;
        let user_ref = r#"{"@entity":"_elysiandb_core_user","id":"admin"}"#;

        s.must_do(200, "POST", "/api/posts", r#"{"id":"p1","title":"t"}"#, &[]);
        s.must_do(
            403,
            "POST",
            "/api/posts",
            format!(r#"{{"id":"p2","x":{hook_ref}}}"#),
            &[],
        );
        s.must_do(
            403,
            "POST",
            "/api/posts",
            format!(r#"[{{"id":"p3","items":[{user_ref}]}}]"#),
            &[],
        );
        s.must_do(
            403,
            "PUT",
            "/api/posts/p1",
            format!(r#"{{"owner":{user_ref}}}"#),
            &[],
        );
        s.must_do(
            403,
            "PUT",
            "/api/posts",
            format!(r#"[{{"id":"p1","meta":{{"deep":{hook_ref}}}}}]"#),
            &[],
        );

        let tx = begin_transaction(&s);
        s.must_do(
            403,
            "POST",
            &format!("/api/tx/{tx}/entity/posts"),
            format!(r#"{{"id":"p4","x":{hook_ref}}}"#),
            &[],
        );
        s.must_do(
            403,
            "PUT",
            &format!("/api/tx/{tx}/entity/posts/p1"),
            format!(r#"{{"owner":{user_ref}}}"#),
            &[],
        );
        s.must_do(200, "POST", &format!("/api/tx/{tx}/commit"), "", &[]);

        let hooks = s.must_do(200, "GET", "/api/hook/posts", "", &[]).list();
        if !hooks.is_empty() {
            panic!("{name}: expected no hook to be created through a nested write, got {hooks:?}");
        }

        let post = s.must_do(200, "GET", "/api/posts/p1", "", &[]).object();
        if post.contains_key("owner") || post.get("pwn").is_some_and(|v| !v.is_null()) {
            panic!("{name}: expected the post to be left untouched, got {post:?}");
        }

        s.must_do(404, "GET", "/api/posts/p2", "", &[]);
    }
}

#[test]
fn includes_do_not_resolve_core_entities() {
    let configure = |cfg: &mut Config| cfg.security.authentication.enabled = false;
    let s = start_api_server(Some(&configure));

    let mut leak = map(vec![
        ("id", vs("l1")),
        (
            "u",
            obj(vec![
                ("@entity", vs("_elysiandb_core_user")),
                ("id", vs("admin")),
            ]),
        ),
        ("w", obj(vec![("@entity", vs("writer")), ("id", vs("w1"))])),
    ]);
    api_storage::write_entity(b"leak", &mut leak);
    let mut writer = map(vec![("id", vs("w1")), ("name", vs("W"))]);
    api_storage::write_entity(b"writer", &mut writer);

    for path in [
        "/api/leak/l1?includes=u,w",
        "/api/leak?includes=all",
        "/api/leak?includes=u",
    ] {
        let resp = s.must_do(200, "GET", path, "", &[]);
        if resp.text().contains("password") || resp.text().contains(r#""role""#) {
            panic!("GET {path} resolved a core entity: {}", resp.text());
        }
    }

    let w = as_object(
        s.must_do(200, "GET", "/api/leak/l1?includes=w", "", &[])
            .object()
            .get("w"),
    );
    if w.get("name") != Some(&vs("W")) {
        panic!(
            "expected regular includes to keep working, got {:?}",
            w.get("name")
        );
    }
}

#[test]
fn pre_read_hooks_do_not_scan_the_whole_entity_for_a_page() {
    let s = start_admin_session(Some(&without_strict_schema));

    let mut posts = Vec::with_capacity(2000);
    let mut authors = Vec::with_capacity(2000);
    for i in 0..2000 {
        posts.push(obj(vec![
            ("id", vs(&format!("p{i:03}"))),
            ("authorId", vs(&format!("a{i:03}"))),
            ("rank", vn(f64::from(i))),
        ]));
        authors.push(obj(vec![
            ("id", vs(&format!("a{i:03}"))),
            ("name", vs(&format!("Author {i}"))),
        ]));
    }
    let posts_body = marshal(&Value::Array(posts));
    let authors_body = marshal(&Value::Array(authors));
    s.must_do(200, "POST", "/api/post", posts_body, &[]);
    s.must_do(200, "POST", "/api/author", authors_body, &[]);
    s.must_do(200, "POST", "/api/hook/post", r#"{"name":"join","event":"pre_read","language":"javascript","enabled":true,"bypass_acl":true,"script":"function preRead(ctx){ var r = ctx.query(\"author\", {\"id\": {\"eq\": ctx.entity.authorId}}); ctx.entity.authorName = r.length ? r[0].name : null }"}"#, &[]);

    let elapsed = |method: &str, path: &str, body: &str| -> Duration {
        s.must_do(200, method, path, body, &[]);
        let start = Instant::now();
        s.must_do(200, method, path, body, &[]);
        start.elapsed()
    };

    let full = elapsed(
        "POST",
        "/api/query",
        r#"{"entity":"post","filters":{"authorName":{"eq":"Author*"}}}"#,
    );

    let page = elapsed("POST", "/api/query", r#"{"entity":"post","limit":5}"#);
    let one = elapsed(
        "POST",
        "/api/query",
        r#"{"entity":"post","filters":{"id":{"eq":"p007"}}}"#,
    );
    let first_matches = elapsed(
        "POST",
        "/api/query",
        r#"{"entity":"post","filters":{"authorName":{"eq":"Author*"}},"limit":5}"#,
    );
    let rest = elapsed(
        "GET",
        "/api/post?filter%5BauthorName%5D%5Beq%5D=Author*&limit=5",
        "",
    );

    for (name, d) in [
        ("REST virtual filter with limit", rest),
        ("page", page),
        ("stored filter", one),
        ("virtual filter with limit", first_matches),
    ] {
        if d * 10 > full {
            panic!("{name} took {d:?}, expected far less than a full scan ({full:?})");
        }
    }

    let resp = s
        .must_do(
            200,
            "POST",
            "/api/query",
            r#"{"entity":"post","filters":{"id":{"eq":"p007"}}}"#,
            &[],
        )
        .list();
    if resp.len() != 1 || resp[0].get("authorName") != Some(&vs("Author 7")) {
        panic!("expected the hook to run on the matching document, got {resp:?}");
    }

    let ids = s.must_do(200, "POST", "/api/query", r#"{"entity":"post","filters":{"authorName":{"eq":"Author 1*"}},"sorts":{"rank":"asc"},"offset":2,"limit":2}"#, &[]).ids();
    if ids != ["p011", "p012"] {
        panic!("expected pagination over the virtual matches, got {ids:?}");
    }
}
