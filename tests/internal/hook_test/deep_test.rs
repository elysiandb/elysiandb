use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::hook;
use talos::host::{DocMap, DocValue, MapRef};

use crate::{fmt_field, obj, s, setup, write};

const DEPTH: usize = 9990;

fn deep_json(depth: usize) -> Value {
    let mut v = Value::Number(1.0);
    for _ in 0..depth {
        v = Value::Object(Map::from([("k".to_string(), v)]));
    }
    v
}

fn deep_entity(depth: usize) -> MapRef {
    let mut v = DocValue::Float64(1.0);
    for _ in 0..depth {
        let mut m = DocMap::new();
        m.insert("k".to_string(), v);
        v = DocValue::new_map(m);
    }
    let mut root = DocMap::new();
    root.insert("id".to_string(), DocValue::string("x"));
    root.insert("deep".to_string(), v);
    match &DocValue::new_map(root) {
        DocValue::Map(m) => m.clone(),
        _ => unreachable!(),
    }
}

fn on_small_stack(f: impl FnOnce() + Send + 'static) {
    let h = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(f)
        .expect("spawn");
    if let Err(p) = h.join() {
        std::panic::resume_unwind(p);
    }
}

#[test]
fn deep_entities_go_through_the_output_memo_without_recursion() {
    let _s = setup(true);
    hook::reset_output_memo();
    let before = hook::output_memo().hits;

    on_small_stack(|| {
        let script =
            "function postRead(ctx) { ctx.entity.copy = ctx.entity.deep; ctx.entity.n = 1 }";
        for round in 0..2 {
            let e = deep_entity(DEPTH);
            if let Err(err) = hook::apply_post_read_script("", script, &e, true) {
                panic!("round {round}: {err}");
            }
            assert_eq!(fmt_field(&e, "n"), "1", "round {round}");
            assert!(e.borrow().contains_key("copy"), "round {round}");
            DocValue::Map(e).release_graph();
        }
    });

    assert_eq!(
        hook::output_memo().hits - before,
        1,
        "the second run reuses the stored output"
    );
}

#[test]
fn deep_query_rows_are_copied_without_recursion() {
    let _s = setup(true);
    api::create_entity_type(b"deep").expect("create deep");
    write(
        "deep",
        obj(vec![("id", s("d1")), ("deep", deep_json(DEPTH))]),
    );

    on_small_stack(|| {
        let script = r#"function postRead(ctx) {
  var r = ctx.query("deep", {})[0]
  var v = r.deep
  var n = 0
  while (typeof v === "object") { v = v.k; n++ }
  ctx.entity.n = n
  ctx.entity.s = ctx.sum("deep", {}, "deep.k")
}"#;
        let e = crate::entity(r#"{"id":"x"}"#);
        if let Err(err) = hook::apply_post_read_script("", script, &e, true) {
            panic!("{err}");
        }
        assert_eq!(fmt_field(&e, "n"), DEPTH.to_string());
        assert_eq!(fmt_field(&e, "s"), "0");
    });
}

#[test]
fn deep_rows_built_on_the_calling_thread_do_not_recurse() {
    let _s = setup(true);
    let _ = api::create_entity_type(b"author");
    let _ = api::create_entity_type(b"book");
    write("author", obj(vec![("id", s("a1")), ("name", s("first"))]));
    write(
        "book",
        obj(vec![
            ("id", s("b1")),
            (
                "author",
                Value::Object(obj(vec![("@entity", s("author")), ("id", s("a1"))])),
            ),
            ("deep", deep_json(DEPTH)),
        ]),
    );

    on_small_stack(|| {
        let script = r#"function postRead(ctx) {
  var rows = ctx.query("book", { "author.name": { eq: "first" } })
  var v = rows[0].deep
  var n = 0
  while (typeof v === "object") { v = v.k; n++ }
  ctx.entity.n = n
}"#;
        let e = crate::entity(r#"{"id":"x"}"#);
        if let Err(err) = hook::apply_post_read_script("", script, &e, true) {
            panic!("{err}");
        }
        assert_eq!(fmt_field(&e, "n"), DEPTH.to_string());
    });
}
