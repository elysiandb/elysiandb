use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::Value;
use elysiandb::storage;

use crate::harness::start_admin_session;

#[test]
fn reset_deletes_kv_keys_and_keeps_the_restapi_working() {
    let configure = |cfg: &mut Config| cfg.api.schema.strict = false;
    let s = start_admin_session(Some(&configure));

    s.must_do(200, "POST", "/api/books", r#"{"title":"Dune"}"#, &[]);
    s.must_do(204, "PUT", "/kv/note", "hello", &[]);

    s.must_do(200, "POST", "/reset", "", &[]);

    s.must_do(404, "GET", "/kv/note", "", &[]);

    let books = s.must_do(200, "GET", "/api/books", "", &[]).list();
    if books.len() != 1 {
        panic!("documents must still be listed after a reset, got {books:?}");
    }

    let users = s
        .must_do(200, "GET", "/api/security/user", "", &[])
        .object();
    let count = match users.get("users") {
        Some(Value::Array(list)) => list.len(),
        _ => 0,
    };
    if count == 0 {
        panic!("users must still be listed after a reset");
    }

    s.must_do(200, "POST", "/api/authors", r#"{"name":"Herbert"}"#, &[]);
    s.must_do(200, "POST", "/api/authors", r#"{"name":"Asimov"}"#, &[]);

    let authors = s.must_do(200, "GET", "/api/authors", "", &[]).list();
    if authors.len() != 2 {
        panic!("expected 2 authors, got {authors:?}");
    }

    s.must_do(200, "GET", "/api/acl/admin/authors", "", &[]);
}

#[test]
fn deleting_internal_keys_cannot_break_the_restapi() {
    let configure = |cfg: &mut Config| cfg.api.schema.strict = false;
    let s = start_admin_session(Some(&configure));

    s.must_do(200, "POST", "/api/books", r#"{"title":"Dune"}"#, &[]);

    storage::delete_by_wildcard_key(format!("{}*", globals::API_KEY_PREFIX).as_bytes());
    storage::delete_by_key(globals::api_entity_index_id_key("books").as_bytes());

    s.must_do(200, "POST", "/api/authors", r#"{"name":"Herbert"}"#, &[]);
    s.must_do(200, "POST", "/api/authors", r#"{"name":"Asimov"}"#, &[]);

    let books = s.must_do(200, "GET", "/api/books", "", &[]).list();
    if books.len() != 1 {
        panic!("books are still listed, got {books:?}");
    }
    let authors = s.must_do(200, "GET", "/api/authors", "", &[]).list();
    if authors.len() != 2 {
        panic!("expected 2 authors, got {authors:?}");
    }
}
