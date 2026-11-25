use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::rand::Rand;
use elysiandb::hook::{self, Hook};
use elysiandb::query::{Filters, Ops};
use elysiandb::storage;
use goja::host::MapRef;

use crate::{entity, fmt_field, obj, rng, s, setup, sprint, write};

pub const BUDGET_SCRIPT: &str = r#"function monthsBetween(from, to) {
  var a = from.split("-"), b = to.split("-")
  return (parseInt(b[0], 10) - parseInt(a[0], 10)) * 12 + parseInt(b[1], 10) - parseInt(a[1], 10)
}
function postRead(ctx) {
  var month = ctx.entity.id
  var purchases = ctx.query("purchase", { status: { neq: "annule" } })
  var total = 0, count = 0
  for (var i = 0; i < purchases.length; i++) {
    var p = purchases[i]
    if (!p.budget_month || p.status === "wishlist") { continue }
    var n = p.installments || 1
    if (n > 1) {
      var k = monthsBetween(p.budget_month, month)
      if (k < 0 || k >= n) { continue }
      total += p.installment_amount || 0
    } else {
      if (p.budget_month !== month) { continue }
      total += p.price || 0
    }
    count++
  }
  total += ctx.entity.spent_manual_adjust || 0
  ctx.entity.spent = Math.round(total * 100) / 100
  ctx.entity.remaining = Math.round(((ctx.entity.cap || 0) - total) * 100) / 100
  ctx.entity.purchase_count = count
  ctx.entity.raw_total = total
}"#;

const PROGRESS_SCRIPT: &str = r#"function postRead(ctx) {
  var steps = ctx.query("step", { program_id: { eq: ctx.entity.id } })
  var done = 0, total = 0
  for (var i = 0; i < steps.length; i++) {
    if (steps[i].status === "vu") { done++ }
    if (steps[i].status !== "hors_marathon") { total++ }
  }
  ctx.entity.steps_done = done
  ctx.entity.steps_total = total
  ctx.entity.progress_label = done + "/" + total
}"#;

pub fn month_id(i: i64) -> String {
    format!("20{:02}-{:02}", 20 + i / 12, 1 + i % 12)
}

pub fn random_purchase(r: &mut Rand, id: &str) -> Map {
    let mut doc = obj(vec![
        ("id", s(id)),
        ("budget_month", s(&month_id(r.intn(36)))),
    ]);
    match r.intn(6) {
        0 => doc.insert("status".to_string(), s("annule")),
        1 => doc.insert("status".to_string(), s("wishlist")),
        _ => doc.insert("status".to_string(), s("achete")),
    };

    doc.insert(
        "price".to_string(),
        Value::Number(r.intn(100000) as f64 / 100.0),
    );
    if r.intn(4) == 0 {
        doc.insert(
            "installments".to_string(),
            Value::Number((2 + r.intn(10)) as f64),
        );
        doc.insert(
            "installment_amount".to_string(),
            Value::Number(r.intn(10000) as f64 / 100.0),
        );
    }

    match r.intn(40) {
        0 => {
            doc.remove("price");
        }
        1 => {
            doc.insert("budget_month".to_string(), s(""));
        }
        2 => {
            doc.remove("status");
        }
        3 => {
            doc.insert("budget_month".to_string(), Value::Null);
        }
        _ => {}
    }

    doc
}

pub fn run_budget(month: &str) -> MapRef {
    let out = entity(&format!(
        r#"{{"id":"{month}","cap":500.0,"spent_manual_adjust":1.5}}"#
    ));
    if let Err(err) = hook::apply_post_read_script("", BUDGET_SCRIPT, &out, true) {
        panic!("{err}");
    }

    out
}

fn run_progress(program: &str) -> MapRef {
    let out = entity(&format!(
        r#"{{"id":{}}}"#,
        elysiandb::gocompat::strconv::quote(program.as_bytes())
    ));
    if let Err(err) = hook::apply_post_read_script("", PROGRESS_SCRIPT, &out, true) {
        panic!("{err}");
    }

    out
}

pub fn same_docs(a: &MapRef, b: &MapRef) -> bool {
    let (a, b) = (a.borrow(), b.borrow());
    if a.len() != b.len() {
        return false;
    }

    for (k, v) in a.iter() {
        let other = match b.get(k) {
            Some(x) => sprint(x),
            None => "<nil>".to_string(),
        };
        if other != sprint(v) {
            return false;
        }
    }

    true
}

fn dump(e: &MapRef) -> String {
    e.borrow()
        .iter()
        .map(|(k, v)| format!("{k}:{}", sprint(v)))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn compare_budget(months: i64, label: &str) {
    for i in 0..months {
        let month = month_id(i);

        hook::reset_output_memo();
        hook::set_folds_enabled(true);
        let fast = run_budget(&month);

        hook::reset_output_memo();
        hook::set_folds_enabled(false);
        let slow = run_budget(&month);
        hook::set_folds_enabled(true);

        if !same_docs(&fast, &slow) {
            hook::reset_query_cache();
            hook::reset_output_memo();
            let fresh = run_budget(&month);
            panic!(
                "{label} month {month}: fold {}, script {}, fresh {}, {:?}",
                dump(&fast),
                dump(&slow),
                dump(&fresh),
                hook::folds()
            );
        }
    }
}

#[test]
fn test_budget_fold_answers_like_the_script() {
    let _s = setup(true);
    api::create_entity_type(b"purchase").expect("create purchase");

    let mut r = rng(7);
    for i in 0..1200 {
        let d = random_purchase(&mut r, &format!("p{i:05}"));
        write("purchase", d);
    }

    let before = hook::folds();
    compare_budget(36, "initial");
    let got = hook::folds();
    assert!(
        got.full_passes != before.full_passes,
        "the budget script should run on the view, stats {got:?}"
    );

    for round in 0..40 {
        let id = format!("p{:05}", r.intn(1200));
        match r.intn(4) {
            0 => api::delete_entity_by_id(b"purchase", id.as_bytes()),
            1 => {
                let d = random_purchase(&mut r, &format!("n{round:05}"));
                write("purchase", d);
            }
            _ => {
                let d = random_purchase(&mut r, &id);
                let _ = api::update_entity_by_id(b"purchase", id.as_bytes(), &d);
            }
        }

        compare_budget(6, &format!("round {round}"));
    }

    let stats = hook::folds();
    assert!(
        stats.incremental != 0,
        "writes should advance the kept sums, stats {stats:?}"
    );
}

#[test]
fn test_progress_fold_answers_like_the_script() {
    let _s = setup(true);
    api::create_entity_type(b"step").expect("create step");

    let statuses = ["vu", "a_voir", "hors_marathon"];
    let mut r = rng(11);
    for i in 0..600 {
        let program = format!("Prog-{}", r.intn(30));
        let status = statuses[r.intn(3) as usize];
        write(
            "step",
            obj(vec![
                ("id", s(&format!("s{i:05}"))),
                ("program_id", s(&program)),
                ("status", s(status)),
            ]),
        );
    }

    let check = |label: &str| {
        for i in 0..31 {
            let program = format!("prog-{i}");

            hook::reset_output_memo();
            hook::set_folds_enabled(true);
            let fast = run_progress(&program);

            hook::reset_output_memo();
            hook::set_folds_enabled(false);
            let slow = run_progress(&program);
            hook::set_folds_enabled(true);

            assert!(
                same_docs(&fast, &slow),
                "{label} {program}: fold {}, script {}",
                dump(&fast),
                dump(&slow)
            );
        }
    };

    check("initial");
    for round in 0..30 {
        let id = format!("s{:05}", r.intn(600));
        if round % 5 == 4 {
            api::delete_entity_by_id(b"step", id.as_bytes());
        } else {
            let program = format!("prog-{}", r.intn(30));
            let status = statuses[r.intn(3) as usize];
            let _ = api::update_entity_by_id(
                b"step",
                id.as_bytes(),
                &obj(vec![("program_id", s(&program)), ("status", s(status))]),
            );
        }
        check(&format!("round {round}"));
    }

    let fb = hook::folds();
    assert!(
        fb.views != 0,
        "the progress script should run on a view, stats {fb:?}"
    );
}

#[test]
fn test_fold_falls_back_on_values_it_does_not_handle() {
    let _s = setup(true);
    api::create_entity_type(b"step").expect("create step");

    let docs = [
        obj(vec![
            ("id", s("a")),
            ("program_id", s("p")),
            ("status", s("vu")),
        ]),
        obj(vec![
            ("id", s("b")),
            ("program_id", s("p")),
            (
                "status",
                Value::Object(obj(vec![("nested", Value::Bool(true))])),
            ),
        ]),
        obj(vec![
            ("id", s("c")),
            ("program_id", Value::Number(5.0)),
            ("status", s("vu")),
        ]),
        obj(vec![
            ("id", s("d")),
            ("program_id", Value::Array(vec![s("p"), s("q")])),
            ("status", s("vu")),
        ]),
        obj(vec![("id", s("e")), ("status", s("vu"))]),
    ];
    for d in docs {
        write("step", d);
    }

    for program in ["p", "5", "p,q", "P", "*", "2026-01-01", ""] {
        hook::reset_output_memo();
        hook::set_folds_enabled(true);
        let fast = run_progress(program);

        hook::reset_output_memo();
        hook::set_folds_enabled(false);
        let slow = run_progress(program);
        hook::set_folds_enabled(true);

        assert!(
            same_docs(&fast, &slow),
            "{program:?}: fold {}, script {}",
            dump(&fast),
            dump(&slow)
        );
    }
}

#[test]
fn test_kept_results_do_not_survive_their_engine() {
    let script = r#"function postRead(ctx) {
  ctx.entity.ids = ctx.query("twice", {}).map(function (d) { return d.id }).join(",")
}"#;
    let run = || -> String {
        hook::reset_output_memo();
        let out = entity(r#"{"id":"x"}"#);
        if let Err(err) = hook::apply_post_read_script("", script, &out, true) {
            panic!("{err}");
        }
        fmt_field(&out, "ids")
    };

    {
        let _s = setup(true);
        let _ = api::create_entity_type(b"twice");
        write("twice", obj(vec![("id", s("old"))]));
        let got = run();
        assert_eq!(got, "old", "first engine sees {got:?}");
    }

    let _s = setup(true);
    let _ = api::create_entity_type(b"twice");
    write("twice", obj(vec![("id", s("a"))]));
    write("twice", obj(vec![("id", s("b"))]));
    let got = run();
    assert_eq!(got, "a,b", "second engine sees {got:?}");
}

#[test]
fn test_fold_stays_exact_under_concurrent_writes() {
    let _s = setup(true);
    api::create_entity_type(b"purchase").expect("create purchase");

    let mut r = rng(3);
    for i in 0..800 {
        let d = random_purchase(&mut r, &format!("p{i:05}"));
        write("purchase", d);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut w = rng(5);
            while !stop.load(Ordering::SeqCst) {
                let id = format!("p{:05}", w.intn(800));
                let d = random_purchase(&mut w, &id);
                let _ = api::update_entity_by_id(b"purchase", id.as_bytes(), &d);
            }
        })
    };

    for i in 0..200 {
        hook::reset_output_memo();
        let out = run_budget(&month_id(i % 36));
        assert!(
            out.borrow().contains_key("spent"),
            "no result {}",
            dump(&out)
        );
    }

    stop.store(true, Ordering::SeqCst);
    writer.join().expect("writer");

    compare_budget(36, "after the writers");
}

#[test]
fn test_results_follow_the_entities_reached_by_their_filters() {
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
        ]),
    );

    let script = r#"function postRead(ctx) {
  ctx.entity.n = ctx.query("book", { "author.name": { eq: "first" } }).length
}"#;
    let run = || -> String {
        hook::reset_output_memo();
        let out = entity(r#"{"id":"x"}"#);
        if let Err(err) = hook::apply_post_read_script("", script, &out, true) {
            panic!("{err}");
        }
        fmt_field(&out, "n")
    };

    let got = run();
    assert_eq!(got, "1", "the book is found through its author, got {got}");

    let _ = api::update_entity_by_id(b"author", b"a1", &obj(vec![("name", s("second"))]));
    let got = run();
    assert_eq!(
        got, "0",
        "a change of the author must be seen at once, got {got}"
    );
}

#[test]
fn test_equality_keys_cover_every_kind_of_value() {
    let _s = setup(true);
    let _ = api::create_entity_type(b"mixed");

    let values: Vec<Value> = vec![
        s("a"),
        s("A"),
        s("1"),
        s("true"),
        s("x"),
        Value::Number(1.0),
        Value::Number(0.0),
        Value::Bool(true),
        Value::Bool(false),
        Value::Null,
        Value::Array(vec![s("a")]),
        Value::Array(vec![s("x"), s("y")]),
        Value::Array(vec![s("1")]),
        s("2026-01-01"),
        Value::Object(obj(vec![("a", Value::Number(1.0))])),
    ];
    let filters = [
        "a",
        "A",
        "1",
        "0",
        "true",
        "false",
        "x",
        "x,y",
        "y,x",
        "2026-01-01",
        "",
    ];
    let mut r = rng(9);

    let mut ids: Vec<String> = Vec::new();
    for i in 0..80 {
        let id = format!("m{i:03}");
        ids.push(id.clone());
        let v = values[r.intn(values.len() as i64) as usize].clone();
        write("mixed", obj(vec![("id", s(&id)), ("f", v)]));
    }

    let script = r#"function postRead(ctx) {
  ctx.entity.ids = ctx.query("mixed", { f: { eq: ctx.entity.want } }).map(function (d) { return d.id }).join(",")
}"#;
    let run = |f: &str| -> String {
        hook::reset_output_memo();
        let out = entity(&format!(
            r#"{{"id":"x","want":{}}}"#,
            elysiandb::gocompat::strconv::quote(f.as_bytes())
        ));
        if let Err(err) = hook::apply_post_read_script("", script, &out, true) {
            panic!("{err}");
        }
        fmt_field(&out, "ids")
    };

    let check = |ids: &[String]| {
        for f in filters {
            let kept = run(f);

            let fl: Filters = Filters::from([(
                b"f".to_vec(),
                Ops::from([(b"eq".to_vec(), f.as_bytes().to_vec())]),
            )]);
            let mut want: Vec<String> = Vec::new();
            for id in ids {
                if let Some(d) = api::read_entity_by_id(b"mixed", id.as_bytes())
                    && api::filters_match_entity(&d, &fl)
                {
                    want.push(id.clone());
                }
            }

            let mut got: Vec<String> = if kept.is_empty() {
                Vec::new()
            } else {
                kept.split(',').map(|x| x.to_string()).collect()
            };
            got.sort();
            assert!(
                got.join(",") == want.join(","),
                "eq {f:?}: kept result {kept:?}, documents {:?}",
                want.join(",")
            );
        }

        for f in filters {
            let kept = run(f);
            hook::reset_query_cache();
            let fresh = run(f);
            assert_eq!(
                fresh, kept,
                "eq {f:?}: kept result {kept:?}, new result {fresh:?}"
            );
        }
    };

    check(&ids);
    for round in 0..200 {
        let id = ids[r.intn(ids.len() as i64) as usize].clone();
        if r.intn(8) == 0 {
            api::delete_entity_by_id(b"mixed", id.as_bytes());
            let v = values[r.intn(values.len() as i64) as usize].clone();
            write("mixed", obj(vec![("id", s(&id)), ("f", v)]));
        } else {
            let v = values[r.intn(values.len() as i64) as usize].clone();
            let _ = api::update_entity_by_id(b"mixed", id.as_bytes(), &obj(vec![("f", v)]));
        }
        if round % 10 == 0 {
            check(&ids);
        }
    }
    check(&ids);
}

#[test]
fn test_views_follow_writes_before_any_read() {
    let _s = setup(true);
    storage::open(storage::MODE_SERVER).expect("open");
    hook::init_hooks();
    let _ = api::create_entity_type(b"purchase");

    let mut h = Hook {
        name: "budget".to_string(),
        entity: "month".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        enabled: true,
        bypass_acl: true,
        script: BUDGET_SCRIPT.to_string(),
        ..Hook::default()
    };
    hook::create_hook(&mut h).expect("create hook");
    hook::prepare_indexes();

    let mut r = rng(1);
    for i in 0..2000 {
        let d = random_purchase(&mut r, &format!("p{i:05}"));
        write("purchase", d);
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    while hook::folds().bytes == 0 {
        assert!(
            Instant::now() <= deadline,
            "the view should be built without a read, stats {:?}",
            hook::folds()
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    std::thread::sleep(Duration::from_millis(600));

    let before = hook::query_cache();
    let out = run_budget(&month_id(1));
    let after = hook::query_cache();
    assert!(
        after.misses == before.misses && after.hits != before.hits,
        "the first read should find the view ready: {before:?} -> {after:?}"
    );

    hook::set_folds_enabled(false);
    hook::reset_output_memo();
    let slow = run_budget(&month_id(1));
    hook::set_folds_enabled(true);
    assert!(
        same_docs(&out, &slow),
        "view {}, script {}",
        dump(&out),
        dump(&slow)
    );
}
