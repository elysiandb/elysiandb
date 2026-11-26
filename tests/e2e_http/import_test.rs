use elysiandb::gocompat::json::{self, Map, Value};

use crate::harness::{obj, start_admin_session, vn, vs};

#[test]
fn import_rejects_invalid_documents_without_writing_any() {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "POST",
        "/api/book/create",
        r#"{"fields":{"title":{"type":"string","required":true}}}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"id":"old","title":"Kept"}"#,
        &[],
    );

    let resp = s
        .must_do(
            400,
            "POST",
            "/api/import",
            r#"{"book":[{"id":"ok","title":"Dune"},{"id":"ko","year":1965}],"other":[{"id":"o1"}]}"#,
            &[],
        )
        .object();
    let rejected = match resp.get("rejected") {
        Some(Value::Array(list)) => list.clone(),
        _ => Vec::new(),
    };
    if rejected.len() != 1 {
        panic!("expected one rejected document, got {resp:?}");
    }

    let first = rejected[0].as_object().cloned().unwrap_or_default();
    if first.get("entity") != Some(&vs("book"))
        || first.get("id") != Some(&vs("ko"))
        || first.get("index") != Some(&vn(1.0))
    {
        panic!("expected the rejected document to be identified, got {first:?}");
    }

    let ids = s.must_do(200, "GET", "/api/book", "", &[]).ids();
    if ids != ["old"] {
        panic!("a rejected import must not change the entity, got {ids:?}");
    }

    s.must_do(404, "GET", "/api/book/ok", "", &[]);
    s.must_do(404, "GET", "/api/other/o1", "", &[]);
}

#[test]
fn import_replaces_the_documents_of_an_entity() {
    let s = start_admin_session(None);

    s.must_do(200, "POST", "/api/full", r#"{"id":"f1","n":1}"#, &[]);
    s.must_do(200, "POST", "/api/full", r#"{"id":"f2","n":2}"#, &[]);
    s.must_do(200, "POST", "/api/untouched", r#"{"id":"u1"}"#, &[]);
    s.must_do(200, "GET", "/api/full/f2", "", &[]);
    s.must_do(200, "GET", "/api/full", "", &[]);

    s.must_do(
        200,
        "POST",
        "/api/import",
        r#"{"full":[{"id":"f1","n":10}]}"#,
        &[],
    );

    let ids = s.must_do(200, "GET", "/api/full", "", &[]).ids();
    if ids != ["f1"] {
        panic!("expected only the imported documents to be listed, got {ids:?}");
    }

    let count = s
        .must_do(200, "GET", "/api/full/count", "", &[])
        .object()
        .get("count")
        .cloned();
    if count != Some(vn(1.0)) {
        panic!("expected a count of 1, got {count:?}");
    }

    s.must_do(404, "GET", "/api/full/f2", "", &[]);
    let got = s
        .must_do(200, "GET", "/api/full/f1", "", &[])
        .object()
        .get("n")
        .cloned();
    if got != Some(vn(10.0)) {
        panic!("expected the imported version of f1, got {got:?}");
    }

    s.must_do(200, "GET", "/api/untouched/u1", "", &[]);
}

#[test]
fn export_leaves_accounts_and_acls_out() {
    let s = start_admin_session(None);
    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"alice","password":"alice-pw","role":"admin"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"id":"b1","title":"Dune"}"#,
        &[],
    );
    s.must_do(200, "POST", "/api/hook/book", r#"{"name":"tag","event":"post_read","language":"javascript","enabled":true,"script":"function postRead(ctx) {}"}"#, &[]);

    let dump = s.must_do(200, "GET", "/api/export", "", &[]);
    if dump.text().contains("$2a$") {
        panic!(
            "the export must not contain password hashes: {}",
            dump.text()
        );
    }

    let entities = dump.object();
    for entity in ["_elysiandb_core_user", "_elysiandb_core_acl"] {
        if entities.contains_key(entity) {
            panic!("the export must not contain {entity}");
        }
    }

    for entity in ["book", "_elysiandb_core_hook"] {
        if !entities.contains_key(entity) {
            panic!("expected {entity} in the export, got {entities:?}");
        }
    }
}

#[test]
fn import_never_touches_accounts_and_acls() {
    let source = start_admin_session(None);
    source.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"id":"b1","title":"Dune"}"#,
        &[],
    );

    let mut dump = Map::new();
    let exported = source.must_do(200, "GET", "/api/export", "", &[]).body;
    if let Err(err) = json::unmarshal(&exported, &mut dump) {
        panic!("invalid export: {err}");
    }

    dump.insert(
        "_elysiandb_core_user".to_string(),
        Value::Array(vec![
            obj(vec![
                ("id", vs("admin")),
                ("username", vs("admin")),
                ("password", vs("$2a$10$fromanotherinstance")),
                ("role", vs("admin")),
            ]),
            obj(vec![
                ("id", vs("alice")),
                ("username", vs("alice")),
                ("password", vs("$2a$10$fromanotherinstance")),
                ("role", vs("admin")),
            ]),
        ]),
    );
    dump.insert(
        "_elysiandb_core_acl".to_string(),
        Value::Array(vec![obj(vec![
            ("id", vs("alice::book")),
            ("username", vs("alice")),
            ("entity", vs("book")),
            ("permissions", obj(vec![("read", Value::Bool(true))])),
        ])]),
    );
    let body = json::marshal(&dump).unwrap_or_default();

    let target = start_admin_session(None);
    target.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"bob","password":"bob-pass","role":"admin"}"#,
        &[],
    );
    target.must_do(200, "POST", "/api/movie", r#"{"id":"m1"}"#, &[]);
    target.must_do(200, "PUT", "/api/acl/bob/movie", r#"{"permissions":{"read":true,"create":false,"update":false,"delete":false,"owning_read":false,"owning_write":false,"owning_update":false,"owning_delete":false}}"#, &[]);
    let acls_before = target
        .must_do(200, "GET", "/api/acl/bob/movie", "", &[])
        .text();

    let resp = target
        .must_do(200, "POST", "/api/import", body, &[])
        .object();
    let ignored_ok = resp.get("ignored")
        == Some(&Value::Array(vec![
            vs("_elysiandb_core_acl"),
            vs("_elysiandb_core_user"),
        ]));
    let message = match resp.get("message") {
        Some(Value::String(m)) => m.clone(),
        other => panic!("message is not a string: {other:?}"),
    };
    if !ignored_ok || !message.contains("backup and restore") {
        panic!("expected the import to report the ignored entities, got {resp:?}");
    }

    let anonymous = target.anonymous();
    for (username, password, status) in [
        ("bob", "bob-pass", 200),
        ("admin", "admin", 200),
        ("alice", "alice-pw", 401),
    ] {
        let mut credentials = Map::new();
        credentials.insert("password".to_string(), vs(password));
        credentials.insert("username".to_string(), vs(username));
        let credentials = json::marshal(&credentials).unwrap_or_default();
        anonymous.must_do(status, "POST", "/api/security/login", credentials, &[]);
    }

    let got = target
        .must_do(200, "GET", "/api/acl/bob/movie", "", &[])
        .text();
    if got != acls_before {
        panic!("the import must not change existing ACLs:\nbefore {acls_before}\nafter  {got}");
    }

    target.must_do(200, "GET", "/api/book/b1", "", &[]);
    let acl = target
        .must_do(200, "GET", "/api/acl/bob/book", "", &[])
        .object();
    if acl.get("username") != Some(&vs("bob")) {
        panic!("expected an ACL for the imported entity type, got {acl:?}");
    }

    target.must_do(404, "GET", "/api/acl/alice/book", "", &[]);

    target.must_do(
        200,
        "POST",
        "/api/strict/create",
        r#"{"fields":{"title":{"type":"string","required":true}}}"#,
        &[],
    );
    let rejected = target
        .must_do(
            400,
            "POST",
            "/api/import",
            r#"{"_elysiandb_core_user":[],"strict":[{"id":"s1"}]}"#,
            &[],
        )
        .object();
    let ignored_ok =
        rejected.get("ignored") == Some(&Value::Array(vec![vs("_elysiandb_core_user")]));
    let has_rejected = rejected.get("rejected").is_some_and(|v| !v.is_null());
    if !ignored_ok || !has_rejected {
        panic!("expected a rejected import to report the ignored entities too, got {rejected:?}");
    }
}
