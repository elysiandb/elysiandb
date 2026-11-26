use std::panic::{self, AssertUnwindSafe};

use elysiandb::api;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::httpd::{Handler, Request, RequestCtx, handler};
use elysiandb::transport::http::api as http_api;

use super::common::{read_jsonl, str_field, unhex};
use super::{GoldenStore, go_decode, go_map, open_store};

const N3_GO: &str = "expected type []map[string]interface {} but got array";
const N3_RUST: &str = "expected type array of included documents but got array";

const WRITE_HANDLERS: [&str; 8] = [
    "create",
    "update",
    "update_list",
    "delete",
    "destroy",
    "migrate",
    "put_schema",
    "create_type",
];

fn handler_for(name: &str) -> Handler {
    let core = |f: fn(&mut RequestCtx)| http_api::reject_core_entity(handler(f));
    match name {
        "list" => core(http_api::list_controller),
        "query" => handler(http_api::query_controller),
        "count" => core(http_api::count_controller),
        "exists" => core(http_api::exists_controller),
        "get" => core(http_api::get_by_id_controller),
        "create" => core(http_api::create_controller),
        "update" => core(http_api::update_by_id_controller),
        "update_list" => core(http_api::update_list_controller),
        "delete" => core(http_api::delete_by_id_controller),
        "destroy" => core(http_api::destroy_controller),
        "migrate" => core(http_api::migrate_controller),
        "put_schema" => core(http_api::put_schema_controller),
        "get_schema" => core(http_api::get_schema_controller),
        "create_type" => core(http_api::create_type_controller),
        "types" => handler(http_api::get_entity_types_controller),
        "types_names" => handler(http_api::get_entity_types_names_controller),
        other => panic!("unknown handler {other}"),
    }
}

struct UuidMasker {
    seen: Vec<(Vec<u8>, String)>,
}

fn is_hex(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
}

fn is_uuid_v4(b: &[u8]) -> bool {
    if b.len() < 36 {
        return false;
    }
    for (i, &c) in b[..36].iter().enumerate() {
        let ok = match i {
            8 | 13 | 18 | 23 => c == b'-',
            14 => c == b'4',
            19 => matches!(c, b'8' | b'9' | b'a' | b'b'),
            _ => is_hex(c),
        };
        if !ok {
            return false;
        }
    }
    true
}

impl UuidMasker {
    fn mask(&mut self, b: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if is_uuid_v4(&b[i..]) {
                let u = b[i..i + 36].to_vec();
                let name = match self.seen.iter().find(|(k, _)| *k == u) {
                    Some((_, name)) => name.clone(),
                    None => {
                        let name = format!("<uuid#{}>", self.seen.len() + 1);
                        self.seen.push((u, name.clone()));
                        name
                    }
                };
                out.extend_from_slice(name.as_bytes());
                i += 36;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }
}

fn doc(text: &str) -> Map {
    go_map(text.as_bytes())
}

fn write(entity: &str, text: &str) {
    let errs = api::write_entity(entity.as_bytes(), &mut doc(text));
    assert!(errs.is_empty(), "seed {entity} {text}: {errs:?}");
}

fn seed_books() {
    write("person", r#"{"id":"p1","name":"Frank"}"#);
    write(
        "book",
        r#"{"id":"b1","title":"Dune","year":1965,"tags":["sf","classic"],"author":{"@entity":"person","id":"p1"}}"#,
    );
    write(
        "book",
        r#"{"id":"b2","title":"dune messiah","year":"1969","tags":[],"author":{"@entity":"person","id":"p9"}}"#,
    );
    write("book", r#"{"id":"b3","title":"Emma","flag":false}"#);
}

fn seed_teams() {
    write("person", r#"{"id":"p1","name":"Ann"}"#);
    write("person", r#"{"id":"p2","name":"Bob"}"#);
    write(
        "team",
        r#"{"id":"t1","name":"Core","lead":{"@entity":"person","id":"p1"},"members":[{"@entity":"person","id":"p1"},{"@entity":"person","id":"p2"}]}"#,
    );
}

fn open_set(set: &str) -> GoldenStore {
    match set {
        "books" => {
            let store = open_store(false, false, false);
            seed_books();
            store
        }
        "books-cache" => {
            let store = open_store(false, false, true);
            seed_books();
            store
        }
        "teams" => {
            let store = open_store(true, false, false);
            seed_teams();
            store
        }
        "schema" => open_store(true, false, false),
        "strict" => open_store(true, true, false),
        other => panic!("unknown set {other}"),
    }
}

fn call(row: &Value) -> (RequestCtx, bool) {
    let mut req = Request::new();
    req.set_request_uri(str_field(row, "uri"));
    req.header.set_method(str_field(row, "method"));
    let body = request_body(row);
    if !body.is_empty() {
        req.set_body(&body);
    }
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    let entity = str_field(row, "entity");
    if !entity.is_empty() {
        ctx.set_param("entity", entity);
    }
    let id = str_field(row, "id");
    if !id.is_empty() {
        ctx.set_param("id", id);
    }

    let h = handler_for(str_field(row, "handler"));
    let panicked = panic::catch_unwind(AssertUnwindSafe(|| h(&mut ctx))).is_err();
    if panicked {
        ctx.response.reset();
        ctx.response.header.set("Content-Type", "application/json");
        ctx.set_status_code(500);
    }

    (ctx, panicked)
}

fn request_body(row: &Value) -> Vec<u8> {
    match row.get("request_body_hex") {
        Some(Value::String(h)) => unhex(h),
        _ => str_field(row, "request_body").as_bytes().to_vec(),
    }
}

fn text_or_hex(row: &Value, field: &str) -> Vec<u8> {
    match row.get(&format!("{field}_hex")) {
        Some(Value::String(h)) => unhex(h),
        _ => str_field(row, field).as_bytes().to_vec(),
    }
}

fn validation_errors(body: &[u8]) -> Option<Vec<(String, String)>> {
    let mut list: Vec<Map> = Vec::new();
    if json::unmarshal(body, &mut list).is_err() || list.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for e in &list {
        let (Some(Value::String(f)), Some(Value::String(m))) = (e.get("Field"), e.get("Message"))
        else {
            return None;
        };
        if e.len() != 2 {
            return None;
        }
        out.push((f.clone(), m.clone()));
    }
    out.sort();
    Some(out)
}

fn stored_entity(entity: &str, m: &mut UuidMasker) -> Value {
    let docs = api::list_entities(entity.as_bytes(), 0, 0, b"", true, None, b"", b"");
    let mut out = Vec::new();
    for d in &docs {
        let id = match d.get("id") {
            Some(Value::String(id)) => id.clone(),
            _ => String::new(),
        };
        let Some(b) = api::read_entity_bytes_by_id(entity.as_bytes(), id.as_bytes()) else {
            continue;
        };
        let mut row = Map::new();
        row.insert(
            "id".to_string(),
            Value::String(String::from_utf8_lossy(&m.mask(id.as_bytes())).into_owned()),
        );
        row.insert(
            "bytes".to_string(),
            Value::String(String::from_utf8(m.mask(&b)).expect("utf-8 document")),
        );
        out.push(Value::Object(row));
    }
    if out.is_empty() {
        return Value::Null;
    }
    Value::Array(out)
}

fn check_row(row: &Value, m: &mut UuidMasker) {
    let name = format!("{}/{}", str_field(row, "set"), str_field(row, "name"));
    let (ctx, panicked) = call(row);

    let status = match row.get("status") {
        Some(Value::Int(n)) => *n,
        other => panic!("{name}: no status {other:?}"),
    };
    assert_eq!(
        i64::from(ctx.response.status_code()),
        status,
        "{name}: status, body {}",
        String::from_utf8_lossy(ctx.response.body_bytes())
    );
    assert_eq!(
        String::from_utf8_lossy(ctx.response.header.content_type()),
        str_field(row, "content_type"),
        "{name}: content type"
    );
    assert_eq!(
        String::from_utf8_lossy(ctx.response.header.peek("X-Elysian-Cache")),
        str_field(row, "x_elysian_cache"),
        "{name}: cache header"
    );
    assert_eq!(
        Some(&Value::Bool(panicked)),
        row.get("panic"),
        "{name}: panic"
    );

    let body = ctx.response.body_bytes().to_vec();
    if row.get("message_kind").and_then(Value::as_str) == Some("go") {
        let decoded = go_decode(&body);
        let message = decoded
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{name}: no error in {}", String::from_utf8_lossy(&body)));
        let field = str_field(row, "go_field");
        assert!(
            message.starts_with(&format!("invalid JSON: \"{field}\"")),
            "{name}: {message}"
        );
    } else if let Some(Value::Array(want)) = row.get("validation_errors_sorted") {
        let want: Vec<(String, String)> = want
            .iter()
            .map(|pair| match pair {
                Value::Array(p) if p.len() == 2 => (
                    p[0].as_str().unwrap_or_default().to_string(),
                    p[1].as_str().unwrap_or_default().to_string(),
                ),
                other => panic!("{name}: bad pair {other:?}"),
            })
            .collect();
        assert_eq!(
            validation_errors(&body),
            Some(want),
            "{name}: validation errors"
        );
    } else {
        assert!(
            validation_errors(&body).is_none(),
            "{name}: unexpected error list"
        );
        assert_eq!(
            String::from_utf8_lossy(&m.mask(&body)),
            String::from_utf8_lossy(&text_or_hex(row, "body")).replace(N3_GO, N3_RUST),
            "{name}: body"
        );
    }

    let entity = str_field(row, "entity");
    if WRITE_HANDLERS.contains(&str_field(row, "handler")) && !entity.is_empty() {
        let got = stored_entity(entity, m);
        let want = row.get("stored").cloned().unwrap_or(Value::Null);
        assert_eq!(
            String::from_utf8_lossy(&json::marshal(&got).expect("encode")),
            String::from_utf8_lossy(&json::marshal(&want).expect("encode")),
            "{name}: stored documents"
        );

        let schema = api::read_entity_bytes_by_id(b"_elysiandb_core_schema", entity.as_bytes())
            .map(|b| m.mask(&b));
        let want = row
            .get("schema_document")
            .map(|_| text_or_hex(row, "schema_document"));
        assert_eq!(
            schema.as_deref().map(String::from_utf8_lossy),
            want.as_deref().map(String::from_utf8_lossy),
            "{name}: schema document"
        );
    }
}

#[test]
fn rest_corpus_goes_through_the_controllers() {
    let rows = read_jsonl("api/rest_corpus.jsonl");
    let mut masker = UuidMasker { seen: Vec::new() };
    let mut current: Option<(String, GoldenStore)> = None;
    let mut checked = 0;

    for row in &rows {
        let set = str_field(row, "set").to_string();
        if current.as_ref().map(|(s, _)| s.as_str()) != Some(set.as_str()) {
            drop(current.take());
            current = Some((set.clone(), open_set(&set)));
        }
        check_row(row, &mut masker);
        checked += 1;
    }

    assert_eq!(checked, 145);
    assert_eq!(checked, rows.len());
}
