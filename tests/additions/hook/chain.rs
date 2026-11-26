use elysiandb::api;
use elysiandb::hook::{self, Hook};

use super::{entity, fmt_field, obj, s, setup, write};

fn chain_hook(name: &str, priority: i64, script: &str) -> Hook {
    Hook {
        entity: "chain".to_string(),
        name: name.to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        enabled: true,
        bypass_acl: true,
        priority,
        script: script.to_string(),
        ..Hook::default()
    }
}

#[test]
fn query_views_stored_by_one_hook_are_read_again_by_the_next() {
    let _s = setup(true);
    hook::init_hooks();
    api::create_entity_type(b"order").expect("create order");
    for i in 0..3 {
        write(
            "order",
            obj(vec![("id", s(&format!("o{i}"))), ("group", s("g"))]),
        );
    }

    hook::create_hook(&mut chain_hook(
        "first",
        10,
        r#"function postRead(ctx) {
  ctx.entity.rows = ctx.query("order", {})
  ctx.entity.group = ctx.group("order", {}, "group")
  ctx.entity.first = ctx.entity.rows.Get(0).id
}"#,
    ))
    .expect("first hook");
    hook::create_hook(&mut chain_hook(
        "second",
        1,
        r#"function postRead(ctx) {
  var x = ctx.entity.rows.Get(0)
  var junk = []
  for (var i = 0; i < 20000; i++) { junk.push({ i: i }) }
  ctx.entity.second = x.id + ":" + ctx.entity.rows.Get(1).id + ":" + ctx.entity.rows.Len() + ":" + ctx.entity.group.Get("g").count
  delete ctx.entity.rows
  delete ctx.entity.group
}"#,
    ))
    .expect("second hook");

    let out = hook::apply_post_read_hooks_for_entity("", "chain", &entity(r#"{"id":"x"}"#));
    assert_eq!(fmt_field(&out, "first"), "o0");
    assert_eq!(fmt_field(&out, "second"), "o0:o1:3:3");
}

#[test]
fn dotted_filters_reach_plain_nested_objects() {
    let _s = setup(true);
    api::create_entity_type(b"deep").expect("create deep");
    write(
        "deep",
        obj(vec![
            ("id", s("d1")),
            (
                "meta",
                elysiandb::gocompat::json::Value::Object(obj(vec![("name", s("a"))])),
            ),
        ]),
    );
    write(
        "deep",
        obj(vec![
            ("id", s("d2")),
            (
                "meta",
                elysiandb::gocompat::json::Value::Object(obj(vec![("name", s("b"))])),
            ),
        ]),
    );

    let e = entity(r#"{"id":"x"}"#);
    let script = r#"function postRead(ctx) {
  var rows = ctx.query("deep", { "meta.name": { eq: "a" } })
  ctx.entity.ids = rows.map(function (r) { return r.id }).join(",")
  ctx.entity.n = ctx.count("deep", { "meta.name": { eq: "a" } })
}"#;
    if let Err(err) = hook::apply_post_read_script("", script, &e, true) {
        panic!("{err}");
    }
    assert_eq!(fmt_field(&e, "ids"), "d1");
    assert_eq!(fmt_field(&e, "n"), "1");
}
