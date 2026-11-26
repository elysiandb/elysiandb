use std::collections::BTreeSet;
use std::sync::LazyLock;

use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};

use super::{doc, docs, filters, setup};

static STORE: LazyLock<Vec<(&'static str, Map)>> = LazyLock::new(|| {
    vec![
        (
            "category:e6a49734",
            doc(r#"{"id":"e6a49734","type":"gogo","@entity":"category"}"#),
        ),
        (
            "job:1234567890",
            doc(r#"{"id":"1234567890","designation":"Worker","@entity":"job"}"#),
        ),
        (
            "author:32e1f608",
            doc(
                r#"{"id":"32e1f608","fullname":"Taymour Negib","status":"writer","job":{"@entity":"job","id":"1234567890"},"@entity":"author"}"#,
            ),
        ),
        (
            "author:25667686",
            doc(
                r#"{"id":"25667686","fullname":"Alberto","status":"coco","job":{"@entity":"job","id":"1234567890"},"@entity":"author"}"#,
            ),
        ),
    ]
});

fn lookup(key: &[u8]) -> Option<Map> {
    STORE
        .iter()
        .find(|(k, _)| k.as_bytes() == key)
        .map(|(_, m)| m.clone())
}

pub fn mock_read_entity_by_id(entity: &[u8], id: &[u8]) -> Option<Map> {
    let mut key = entity.to_vec();
    key.push(b':');
    key.extend_from_slice(id);
    if let Some(v) = lookup(&key) {
        return Some(v);
    }
    if id.len() > 8 {
        let mut key = entity.to_vec();
        key.push(b':');
        key.extend_from_slice(&id[..8]);
        if let Some(v) = lookup(&key) {
            return Some(v);
        }
    }
    None
}

fn normalize_authors(raw: Option<&Value>) -> Vec<Map> {
    match raw {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|el| match el {
                Value::Object(m) => Some(m.clone()),
                _ => None,
            })
            .collect(),
        Some(Value::DocList(maps)) => maps.clone(),
        _ => Vec::new(),
    }
}

fn object<'a>(m: &'a Map, key: &str) -> &'a Map {
    match m.get(key) {
        Some(Value::Object(o)) => o,
        other => panic!("{key} is not an object: {other:?}"),
    }
}

fn text<'a>(m: &'a Map, key: &str) -> Option<&'a str> {
    m.get(key).and_then(Value::as_str)
}

#[test]
fn apply_includes_simple_include() {
    let _s = setup();
    let mut data = docs(
        r#"[{"id":"d5a3e752","name":"youyou","category":{"@entity":"category","id":"e6a49734"}}]"#,
    );
    api::apply_includes(&mut data, b"category");
    let category = object(&data[0], "category");
    if text(category, "type") != Some("gogo") || text(category, "@entity") != Some("category") {
        panic!("unexpected category: {category:?}");
    }
}

#[test]
fn apply_includes_multiple_includes() {
    let _s = setup();
    let mut data = docs(
        r#"[{"id":"d5a3e752","name":"youyou","category":{"@entity":"category","id":"e6a49734"},"authors":[{"@entity":"author","id":"32e1f608"},{"@entity":"author","id":"25667686"}]}]"#,
    );
    api::apply_includes(&mut data, b"category,authors,authors.job");
    let entity = &data[0];
    let cat = object(entity, "category");
    if text(cat, "type") != Some("gogo") {
        panic!("category not included properly: {cat:?}");
    }
    let authors = normalize_authors(entity.get("authors"));
    if authors.len() != 2 {
        panic!("unexpected authors length: {}", authors.len());
    }
    for a in &authors {
        if a.get("fullname").is_none() {
            panic!("author not loaded: {a:?}");
        }
        match a.get("job") {
            Some(Value::Object(job)) if text(job, "designation") == Some("Worker") => {}
            other => panic!("job not loaded properly: {other:?}"),
        }
    }
}

#[test]
fn apply_includes_all_mode() {
    let _s = setup();
    let mut data = docs(
        r#"[{"id":"test1","name":"entity1","category":{"@entity":"category","id":"e6a49734"},"authors":[{"@entity":"author","id":"32e1f608"}]}]"#,
    );
    api::apply_includes(&mut data, b"all");
    let entity = &data[0];
    let cat = object(entity, "category");
    if text(cat, "type") != Some("gogo") {
        panic!("category not included in all mode: {cat:?}");
    }
    let authors = normalize_authors(entity.get("authors"));
    if authors.is_empty() {
        panic!(
            "authors not included in all mode: {:?}",
            entity.get("authors")
        );
    }
    let a = &authors[0];
    if text(a, "fullname") != Some("Taymour Negib") {
        panic!("author not included in all mode: {a:?}");
    }
    let job = object(a, "job");
    if text(job, "designation") != Some("Worker") {
        panic!("nested job not included in all mode: {job:?}");
    }
}

#[test]
fn apply_includes_empty_include() {
    let _s = setup();
    let data = docs(r#"[{"id":"1","name":"simple"}]"#);
    let mut result = data.clone();
    api::apply_includes(&mut result, b"");
    if result != data {
        panic!("expected unchanged data, got {result:?}");
    }
}

#[test]
fn apply_includes_missing_relations() {
    let _s = setup();
    let mut data = docs(r#"[{"id":"x","name":"test","unknown":{"@entity":"missing","id":"000"}}]"#);
    api::apply_includes(&mut data, b"unknown");
    let val = object(&data[0], "unknown");
    if text(val, "@entity") != Some("missing") {
        panic!("unexpected missing entity behavior: {val:?}");
    }
}

#[test]
fn extract_auto_includes_simple() {
    let _s = setup();
    let input = filters(&[("author.name", &[("eq", "John")])]);
    let got = api::extract_auto_includes(&input);
    if got != b"author" {
        panic!("expected author, got {:?}", String::from_utf8_lossy(&got));
    }
}

#[test]
fn extract_auto_includes_multiple() {
    let _s = setup();
    let input = filters(&[
        ("author.name", &[("eq", "John")]),
        ("author.profile.age", &[("eq", "20")]),
        ("category.title", &[("eq", "news")]),
        ("title", &[("eq", "x")]),
    ]);
    let got = api::extract_auto_includes(&input);
    let s: BTreeSet<&[u8]> = got.split(|&c| c == b',').collect();
    if !s.contains(b"author".as_slice())
        || !s.contains(b"author.profile".as_slice())
        || !s.contains(b"category".as_slice())
    {
        panic!(
            "auto includes mismatch: {:?}",
            String::from_utf8_lossy(&got)
        );
    }
}

#[test]
fn merge_includes() {
    let _s = setup();
    let got = api::merge_includes(b"author,profile", b"profile,category");
    let s: BTreeSet<&[u8]> = got.split(|&c| c == b',').collect();
    if !s.contains(b"author".as_slice())
        || !s.contains(b"profile".as_slice())
        || !s.contains(b"category".as_slice())
    {
        panic!("merge mismatch: {:?}", String::from_utf8_lossy(&got));
    }
}
