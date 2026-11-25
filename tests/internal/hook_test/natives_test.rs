use elysiandb::api;
use elysiandb::gocompat::json::Value;
use elysiandb::hook;
use goja::host::DocValue;

use crate::{entity, field, obj, s, setup, write};

#[test]
#[ignore = "requires the JSON builtin (JSON.stringify), ported on another branch"]
fn test_native_aggregates_match_their_java_script_equivalent() {
    let _s = setup(true);
    api::create_entity_type(b"buy").expect("create buy");
    for i in 0..300 {
        let mut doc = obj(vec![
            ("id", s(&format!("b{i:03}"))),
            ("month", s(&format!("2026-{:02}", 1 + i % 12))),
            ("price", Value::Number((i % 17) as f64)),
            ("status", s(["ok", "annule"][i % 5 / 4])),
            (
                "meta",
                Value::Object(obj(vec![("weight", Value::Number((i % 3) as f64))])),
            ),
        ]);
        if i % 7 == 0 {
            doc.remove("price");
        }
        write("buy", doc);
    }

    let script = r#"function postRead(ctx) {
  var f = { status: { neq: "annule" } }
  var rows = ctx.query("buy", f)
  var sum = 0, weight = 0, groups = {}
  for (var i = 0; i < rows.length; i++) {
    var p = rows[i]
    if (typeof p.price === "number") { sum += p.price }
    weight += p.meta.weight
    var g = groups[p.month] || (groups[p.month] = { count: 0, sum: 0 })
    g.count++
    if (typeof p.price === "number") { g.sum += p.price }
  }
  var native = ctx.group("buy", f, "month", "price")
  var same = Object.keys(native).length === Object.keys(groups).length
  Object.keys(groups).forEach(function (k) {
    same = same && native[k].count === groups[k].count && native[k].sum === groups[k].sum
  })
  ctx.entity.count = ctx.count("buy", f) === rows.length
  ctx.entity.sum = ctx.sum("buy", f, "price") === sum
  ctx.entity.nested = ctx.sum("buy", f, "meta.weight") === weight
  ctx.entity.groups = same
  ctx.entity.counts = JSON.stringify(ctx.group("buy", f, "status"))
  ctx.entity.missing = ctx.group("buy", f, "month")["1999-01"] === undefined
}"#;

    let out = entity(r#"{"id":"x"}"#);
    if let Err(err) = hook::apply_post_read_script("", script, &out, true) {
        panic!("{err}");
    }

    for k in ["count", "sum", "nested", "groups", "missing"] {
        assert!(
            matches!(field(&out, k), DocValue::Bool(true)),
            "{k} differs from the JavaScript computation"
        );
    }

    match &field(&out, "counts") {
        DocValue::String(c) if &**c == r#"{"ok":{"count":240}}"# => {}
        _ => panic!("group counts {}", crate::fmt_field(&out, "counts")),
    }
}
