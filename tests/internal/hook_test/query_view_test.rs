use std::sync::Arc;
use std::time::{Duration, Instant};

use elysiandb::api::{self, Guard, GuardError};
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::hook::{self, Hook};
use elysiandb::memory;
use elysiandb::query::{Filters, Ops};
use goja::host::{DocValue, MapRef};

use crate::{entity, field, fmt_field, obj, rng, run_post_read, s, setup, sprint_json, write};

fn seed_orders(n: usize) {
    api::create_entity_type(b"order").expect("create order entity type");

    for i in 0..n {
        write(
            "order",
            obj(vec![
                ("id", s(&format!("o{i}"))),
                ("group", s(&format!("g{}", i % 3))),
                ("amount", Value::Number(i as f64)),
                ("tags", Value::Array(vec![s("a"), s(&format!("t{i}"))])),
                (
                    "owner",
                    Value::Object(obj(vec![
                        ("name", s("n")),
                        ("level", Value::Number((i % 2) as f64)),
                    ])),
                ),
            ]),
        );
    }
}

fn run(script: &str, text: &str) -> MapRef {
    let e = entity(text);
    run_post_read(script, &e);
    e
}

#[test]
fn test_query_results_behave_like_plain_arrays() {
    let _s = setup(true);
    seed_orders(6);

    let script = r#"
function postRead(ctx) {
  var a = ctx.query("order", { group: { eq: "g0" } })
  ctx.entity.isArray = Array.isArray(a)
  ctx.entity.len = a.length
  ctx.entity.typeofRow = typeof a[0]
  ctx.entity.keys = Object.keys(a[0]).sort().join(",")
  ctx.entity.hasOwn = a[0].hasOwnProperty("amount") && !a[0].hasOwnProperty("missing")
  ctx.entity.inOp = ("group" in a[0]) && !("nope" in a[0])
  ctx.entity.missing = a[0].nope === undefined
  a[0].amount = 1000
  a[0].extra = "x"
  delete a[0].group
  ctx.entity.mutated = a[0].amount + ":" + a[0].extra + ":" + (a[0].group === undefined)
  ctx.entity.mutKeys = Object.keys(a[0]).sort().join(",")
  a[0].group = "back"
  ctx.entity.readded = Object.keys(a[0]).filter(function (k) { return k === "group" }).length
  a[1].tags.push("pushed")
  a[1].owner.name = "changed"
  ctx.entity.nested = a[1].tags.length + ":" + a[1].owner.name
  var b = ctx.query("order", { group: { eq: "g0" } })
  ctx.entity.isolated = b[0].amount + ":" + b[0].group + ":" + b[1].tags.length + ":" + b[1].owner.name + ":" + (b[0].extra === undefined)
  a.push({ id: "new" })
  ctx.entity.pushed = a.length + ":" + a[a.length - 1].id + ":" + b.length
  var sorted = b.slice().sort(function (x, y) { return y.amount - x.amount })
  ctx.entity.sorted = sorted.map(function (o) { return o.id }).join(",")
  b.length = 1
  ctx.entity.truncated = b.length + ":" + (b[1] === undefined)
  b.length = 2
  ctx.entity.regrown = b.length + ":" + (b[1] === undefined)
  ctx.entity.json = JSON.stringify(ctx.query("order", { id: { eq: "o3" } }))
  var total = 0
  ctx.query("order", {}).forEach(function (o) { total += o.amount })
  ctx.entity.total = total
  var forin = []
  for (var k in ctx.query("order", { id: { eq: "o0" } })[0]) { forin.push(k) }
  ctx.entity.forin = forin.sort().join(",")
}"#;

    let got = run(script, r#"{"id":"x"}"#);

    let want = [
        ("isArray", "true"),
        ("len", "2"),
        ("typeofRow", "object"),
        ("keys", "amount,group,id,owner,tags"),
        ("hasOwn", "true"),
        ("inOp", "true"),
        ("missing", "true"),
        ("mutated", "1000:x:true"),
        ("mutKeys", "amount,extra,id,owner,tags"),
        ("readded", "1"),
        ("nested", "3:changed"),
        ("isolated", "0:g0:2:n:true"),
        ("pushed", "3:new:2"),
        ("sorted", "o3,o0"),
        ("truncated", "1:true"),
        ("regrown", "2:true"),
        ("total", "15"),
        ("forin", "amount,group,id,owner,tags"),
    ];

    let mut errors = Vec::new();
    for (k, v) in want {
        let g = fmt_field(&got, k);
        if g != v {
            errors.push(format!("{k} = {g:?}, want {v:?}"));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));

    let text = match &field(&got, "json") {
        DocValue::String(t) => t.to_string(),
        _ => String::new(),
    };
    let decoded = json::unmarshal_value(text.as_bytes());
    let ok = match &decoded {
        Ok(Value::Array(items)) => {
            items.len() == 1
                && items[0].get("id") == Some(&s("o3"))
                && items[0].get("amount") == Some(&Value::Number(3.0))
        }
        _ => false,
    };
    assert!(ok, "JSON.stringify of a query result = {text}");
}

#[test]
fn test_query_results_follow_writes() {
    let _s = setup(true);
    seed_orders(4);

    let script = r#"
function postRead(ctx) {
  var total = 0
  var rows = ctx.query("order", { group: { neq: "none" } })
  for (var i = 0; i < rows.length; i++) { total += rows[i].amount }
  ctx.entity.total = total
  ctx.entity.count = rows.length
}"#;

    let first = run(script, r#"{"id":"x"}"#);
    assert!(
        fmt_field(&first, "total") == "6" && fmt_field(&first, "count") == "4",
        "first run = {} {}",
        fmt_field(&first, "total"),
        fmt_field(&first, "count")
    );

    write(
        "order",
        obj(vec![
            ("id", s("o9")),
            ("group", s("g1")),
            ("amount", Value::Number(10.0)),
        ]),
    );
    let second = run(script, r#"{"id":"x"}"#);
    assert!(
        fmt_field(&second, "total") == "16" && fmt_field(&second, "count") == "5",
        "after a write"
    );

    let _ = api::update_entity_by_id(
        b"order",
        b"o0",
        &obj(vec![("amount", Value::Number(100.0))]),
    );
    let third = run(script, r#"{"id":"x"}"#);
    assert_eq!(fmt_field(&third, "total"), "116", "after an update");

    api::delete_entity_by_id(b"order", b"o9");
    let fourth = run(script, r#"{"id":"x"}"#);
    assert!(
        fmt_field(&fourth, "total") == "106" && fmt_field(&fourth, "count") == "4",
        "after a delete"
    );
}

#[test]
fn test_deterministic_outputs_are_reused_until_read_entities_change() {
    let _s = setup(true);
    seed_orders(3);
    hook::reset_output_memo();

    let script = r#"
function postRead(ctx) {
  var rows = ctx.query("order", { group: { eq: ctx.entity.group } })
  ctx.entity.sum = rows.reduce(function (s, o) { return s + o.amount }, 0)
  delete ctx.entity.drop
}"#;

    let before = hook::output_memo();
    let a = run(script, r#"{"id":"x","group":"g1","drop":true}"#);
    let b = run(script, r#"{"id":"x","group":"g1","drop":true}"#);
    let after = hook::output_memo();
    assert_eq!(
        after.hits - before.hits,
        1,
        "expected one reused output, stats {before:?} -> {after:?}"
    );

    assert!(
        fmt_field(&a, "sum") == "1" && fmt_field(&b, "sum") == "1",
        "sums"
    );

    assert!(
        !b.borrow().contains_key("drop"),
        "a reused output must also delete fields"
    );

    write(
        "order",
        obj(vec![
            ("id", s("o7")),
            ("group", s("g1")),
            ("amount", Value::Number(5.0)),
        ]),
    );
    let c = run(script, r#"{"id":"x","group":"g1","drop":true}"#);
    assert_eq!(
        fmt_field(&c, "sum"),
        "6",
        "an output must be recomputed after a write"
    );

    let d = run(script, r#"{"id":"x","group":"g2","drop":true}"#);
    assert_eq!(
        fmt_field(&d, "sum"),
        "2",
        "another input gets its own output"
    );
}

#[test]
fn test_non_deterministic_scripts_are_never_reused() {
    let _s = setup(true);
    seed_orders(1);
    hook::reset_output_memo();

    for script in [
        "function postRead(ctx) { ctx.entity.v = Date.now() }",
        "function postRead(ctx) { ctx.entity.v = new Date().getTime() }",
        "function postRead(ctx) { ctx.entity.v = Date() }",
        "function postRead(ctx) { ctx.entity.v = Math.random() }",
        "function postRead(ctx) { ctx.entity.v = new (new Date(0).constructor)().getTime() }",
        r#"function postRead(ctx) { var D = this["Da" + "te"]; ctx.entity.v = D.now() }"#,
        r#"function postRead(ctx) { var M = this["Ma" + "th"]; ctx.entity.v = M["ran" + "dom"]() }"#,
    ] {
        let before = hook::output_memo();
        run(script, r#"{"id":"x"}"#);
        run(script, r#"{"id":"x"}"#);
        let after = hook::output_memo();
        assert_eq!(
            after.hits, before.hits,
            "{script}: a time or random dependent output was reused"
        );
    }

    let fixed = "function postRead(ctx) { ctx.entity.v = new Date(86400000).toISOString(); ctx.entity.u = Date.UTC(2020, 0, 1); ctx.entity.ok = new Date(0) instanceof Date }";
    let before = hook::output_memo();
    let first = run(fixed, r#"{"id":"x"}"#);
    let second = run(fixed, r#"{"id":"x"}"#);
    let after = hook::output_memo();
    assert_eq!(
        after.hits - before.hits,
        1,
        "a script using only fixed dates should be reused"
    );

    for e in [first, second] {
        assert!(
            fmt_field(&e, "v") == "1970-01-02T00:00:00.000Z"
                && fmt_field(&e, "u") == "1577836800000"
                && matches!(field(&e, "ok"), DocValue::Bool(true)),
            "fixed dates"
        );
    }
}

#[test]
fn test_failing_scripts_are_not_reused() {
    let _s = setup(true);
    seed_orders(1);
    hook::reset_output_memo();

    let script = r#"function postRead(ctx) { ctx.entity.partial = 1; throw new Error("boom") }"#;
    let before = hook::output_memo();
    for _ in 0..2 {
        let e = entity(r#"{"id":"x"}"#);
        assert!(
            hook::apply_post_read_script("", script, &e, true).is_err(),
            "expected the script error"
        );
        assert_eq!(
            fmt_field(&e, "partial"),
            "1",
            "partial mutations stay visible as before"
        );
    }

    let after = hook::output_memo();
    assert_eq!(
        after.hits, before.hits,
        "a failing script must run every time"
    );
}

#[test]
fn test_query_results_stay_exact_across_many_writes() {
    let _s = setup(true);
    api::create_entity_type(b"item").expect("create item");

    let mut r = rng(5);
    let doc = |r: &mut elysiandb::gocompat::rand::Rand, i: i64| -> Map {
        let status = ["ok", "annule", "draft"][r.intn(3) as usize];
        let n = r.intn(100) as f64;
        obj(vec![
            ("id", s(&format!("i{i:05}"))),
            ("status", s(status)),
            ("n", Value::Number(n)),
        ])
    };
    let mut initial: Vec<Map> = Vec::with_capacity(9000);
    for i in 0..9000 {
        initial.push(doc(&mut r, i));
    }
    api::write_list_of_entities(b"item", &mut initial);

    let script = r#"function postRead(ctx) {
  var rows = ctx.query("item", { status: { neq: "annule" } })
  var ids = []
  for (var i = 0; i < rows.length; i++) { ids.push(rows[i].id + ":" + rows[i].n) }
  ctx.entity.ids = ids.join(",")
}"#;
    let filters: Filters = Filters::from([(
        b"status".to_vec(),
        Ops::from([(b"neq".to_vec(), b"annule".to_vec())]),
    )]);
    let mut next: i64 = 9000;
    for step in 0..400 {
        let op = r.intn(10);
        if op < 4 {
            let d = doc(&mut r, next);
            write("item", d);
            next += 1;
        } else if op < 8 {
            let i = r.intn(next);
            let d = doc(&mut r, i);
            write("item", d);
        } else {
            let i = r.intn(next);
            api::delete_entity_by_id(b"item", format!("i{i:05}").as_bytes());
        }

        if step % 7 != 0 {
            continue;
        }

        let want: Vec<String> =
            api::list_entities(b"item", 0, 0, b"", true, Some(&filters), b"", b"")
                .iter()
                .map(|d| format!("{}:{}", sprint_json(d.get("id")), sprint_json(d.get("n"))))
                .collect();

        let out = run(script, &format!(r#"{{"id":"x{step}"}}"#));
        assert!(
            fmt_field(&out, "ids") == want.join(","),
            "step {step}: the hook saw a different result than the query"
        );
    }
}

struct Budget;

impl Drop for Budget {
    fn drop(&mut self) {
        memory::configure(0);
    }
}

#[test]
fn test_oversized_query_results_are_refused_quickly() {
    let _s = setup(true);
    hook::init_hooks();
    api::create_entity_type(b"big").expect("create big");

    let mut docs: Vec<Map> = Vec::with_capacity(3000);
    for i in 0..3000 {
        docs.push(obj(vec![
            ("id", s(&format!("b{i:05}"))),
            ("status", s("ok")),
            ("body", s(&"x".repeat(200))),
        ]));
    }
    api::write_list_of_entities(b"big", &mut docs);

    let mut h = Hook {
        name: "big".to_string(),
        entity: "holder".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        enabled: true,
        bypass_acl: true,
        script: r#"function postRead(ctx) { ctx.entity.n = ctx.query("big", { status: { eq: "ok" } }).length }"#
            .to_string(),
        ..Hook::default()
    };
    hook::create_hook(&mut h).expect("create hook");

    memory::configure(4 << 20);
    let _budget = Budget;
    hook::reset_query_cache();

    for i in 0..2 {
        let guard = Arc::new(Guard::default());
        let start = Instant::now();
        let out = hook::Request::new("")
            .with_guard(guard.clone())
            .post_read("holder", &entity(&format!(r#"{{"id":"x{i}"}}"#)));
        assert!(
            guard.err() == Some(GuardError::Busy) && !out.borrow().contains_key("n"),
            "run {i}: a result larger than its share must be refused as busy, got {:?} and {}",
            guard.err(),
            fmt_field(&out, "n")
        );
        if i == 1 {
            assert!(
                start.elapsed() <= Duration::from_millis(200),
                "a refused result must be refused at once the second time, took {:?}",
                start.elapsed()
            );
        }
    }
}
