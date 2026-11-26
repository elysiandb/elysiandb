use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::rand::Rand;
use elysiandb::hook;

use crate::{entity, fmt_field, obj, rng, s, setup, write};

struct ScriptGen<'a> {
    r: &'a mut Rand,
    locals: Vec<String>,
}

impl ScriptGen<'_> {
    fn intn(&mut self, n: usize) -> usize {
        self.r.intn(n as i64) as usize
    }

    fn leaf(&mut self) -> String {
        match self.intn(9) {
            0..=2 => format!("p.{}", ["a", "b", "c"][self.intn(3)]),
            3 => self.intn(5).to_string(),
            4 => [r#""x""#, r#""2026-03""#, r#""""#, r#""é""#, r#""10""#][self.intn(5)].to_string(),
            5 => "month".to_string(),
            6 => ["null", "undefined", "true", "false", "1.5"][self.intn(5)].to_string(),
            7 => "ctx.entity.adjust".to_string(),
            _ => {
                if !self.locals.is_empty() {
                    let i = self.intn(self.locals.len());
                    return self.locals[i].clone();
                }
                "p.a".to_string()
            }
        }
    }

    fn expr(&mut self, depth: i32) -> String {
        if depth <= 0 {
            return self.leaf();
        }

        match self.intn(12) {
            0..=2 => {
                let op = ["+", "-", "*", "/", "%"][self.intn(5)];
                let a = self.expr(depth - 1);
                let b = self.expr(depth - 1);
                format!("({a} {op} {b})")
            }
            3..=5 => {
                let op = ["===", "!==", "<", "<=", ">", ">=", "==", "!="][self.intn(8)];
                let a = self.expr(depth - 1);
                let b = self.expr(depth - 1);
                format!("({a} {op} {b})")
            }
            6 => {
                let a = self.expr(depth - 1);
                let b = self.expr(depth - 1);
                format!("({a} || {b})")
            }
            7 => {
                let a = self.expr(depth - 1);
                let b = self.expr(depth - 1);
                format!("({a} && {b})")
            }
            8 => format!("!{}", self.expr(depth - 1)),
            9 => {
                let a = self.expr(depth - 1);
                let b = self.expr(depth - 1);
                let c = self.expr(depth - 1);
                format!("({a} ? {b} : {c})")
            }
            10 => {
                let a = format!("typeof {}", self.expr(depth - 1));
                let b = format!("parseInt({}, 10)", self.expr(depth - 1));
                let c = format!("({}).length", self.expr(depth - 1));
                let d = format!("-{}", self.expr(depth - 1));
                [a, b, c, d][self.intn(4)].clone()
            }
            _ => format!("first({})", self.expr(depth - 1)),
        }
    }

    fn body(&mut self) -> String {
        let mut b = String::new();
        let mut i = 0;
        loop {
            let bound = 1 + self.intn(5);
            if i >= bound {
                break;
            }
            match self.intn(6) {
                0 => {
                    let name = format!("v{}", self.locals.len());
                    let e = self.expr(2);
                    b.push_str(&format!("    var {name} = {e}\n"));
                    self.locals.push(name);
                }
                1 => {
                    let e = self.expr(2);
                    b.push_str(&format!("    if ({e}) {{ continue }}\n"));
                }
                2 => {
                    let e = self.expr(2);
                    b.push_str(&format!("    total += {e}\n"));
                }
                3 => {
                    let e1 = self.expr(2);
                    let e2 = self.expr(1);
                    b.push_str(&format!(
                        "    if ({e1}) {{ total -= {e2} }} else {{ count++ }}\n"
                    ));
                }
                4 => b.push_str("    count++\n"),
                _ => {
                    let e = self.expr(1);
                    b.push_str(&format!("    if ({e}) {{ count -= 1 }}\n"));
                }
            }
            i += 1;
        }

        b
    }

    fn script(&mut self, filter: &str) -> String {
        self.locals.clear();
        let body = self.body();
        format!(
            r#"function first(s) {{
  var parts = s.split("-")
  return parts[0]
}}
function postRead(ctx) {{
  var month = ctx.entity.id
  var rows = ctx.query("fuzz", {filter})
  var total = 0, count = 0
  for (var i = 0; i < rows.length; i++) {{
    var p = rows[i]
{body}  }}
  ctx.entity.total = String(total)
  ctx.entity.count = String(count)
}}"#
        )
    }
}

fn fuzz_doc(r: &mut Rand, id: &str) -> Map {
    let value = |r: &mut Rand| -> Value {
        match r.intn(11) {
            0 => Value::Null,
            1 => Value::Bool(true),
            2 => s(""),
            3 => s("2026-03"),
            4 => s("10"),
            5 => s("é-x"),
            6 => Value::Object(obj(vec![("n", Value::Number(1.0))])),
            7 => Value::Array(vec![s("a")]),
            _ => Value::Number(r.intn(1000) as f64 / 8.0),
        }
    };

    let mut doc = obj(vec![
        ("id", s(id)),
        ("group", s(["g1", "g2", "G1"][r.intn(3) as usize])),
    ]);
    for f in ["a", "b", "c"] {
        if r.intn(5) > 0 {
            let v = value(r);
            doc.insert(f.to_string(), v);
        }
    }

    doc
}

fn regular_doc(r: &mut Rand, id: &str) -> Map {
    let mut doc = obj(vec![
        ("id", s(id)),
        ("group", s(["g1", "g2", "G1"][r.intn(3) as usize])),
    ]);
    doc.insert("a".to_string(), Value::Number(r.intn(1000) as f64 / 8.0));
    doc.insert(
        "b".to_string(),
        s(["x", "2026-03", "10", ""][r.intn(4) as usize]),
    );
    if r.intn(3) > 0 {
        doc.insert("c".to_string(), Value::Number(r.intn(4) as f64));
    }

    doc
}

fn run_both(script: &str, id: &str) -> (String, String) {
    let run = |enabled: bool| -> String {
        hook::reset_output_memo();
        hook::set_folds_enabled(enabled);

        let out = entity(&format!(r#"{{"id":"{id}","adjust":2.5}}"#));
        let res = hook::apply_post_read_script("", script, &out, true);
        hook::set_folds_enabled(true);
        if res.is_err() {
            return "error".to_string();
        }
        format!("{}|{}", fmt_field(&out, "total"), fmt_field(&out, "count"))
    };

    (run(true), run(false))
}

#[test]
fn fold_matches_the_script_on_random_scripts() {
    let _s = setup(true);
    api::create_entity_type(b"fuzz").expect("create fuzz");

    let mut r = rng(20261105);
    for i in 0..400 {
        let d = fuzz_doc(&mut r, &format!("d{i:04}"));
        write("fuzz", d);
    }

    let filters = ["{}", r#"{ group: { eq: "g1" } }"#, r#"{ b: { neq: "x" } }"#];

    let check = |r: &mut Rand, label: &str, rounds: usize| {
        for _ in 0..rounds {
            let f = filters[r.intn(filters.len() as i64) as usize];
            let script = ScriptGen {
                r,
                locals: Vec::new(),
            }
            .script(f);
            let (fast, slow) = run_both(&script, "2026-03");
            assert!(
                fast == slow,
                "{label}: fold {fast:?}, script {slow:?}\n{script}"
            );
        }
    };

    check(&mut r, "odd values", 200);

    for i in 0..400 {
        let id = format!("d{i:04}");
        let d = regular_doc(&mut r, &id);
        let _ = api::update_entity_by_id(b"fuzz", id.as_bytes(), &d);
    }

    let before = hook::folds();
    check(&mut r, "regular values", 300);
    let got = hook::folds();
    assert!(
        got.full_passes != before.full_passes,
        "regular values should run on the view, stats {got:?}"
    );

    let scripts = make2(&mut r, &filters, 30);
    for round in 0..60 {
        for script in &scripts {
            let (fast, slow) = run_both(script, "2026-03");
            assert!(
                fast == slow,
                "round {round}: fold {fast:?}, script {slow:?}\n{script}"
            );
        }

        let id = format!("d{:04}", r.intn(400));
        match r.intn(4) {
            0 => api::delete_entity_by_id(b"fuzz", id.as_bytes()),
            1 => {
                let d = regular_doc(&mut r, &format!("n{round:04}"));
                write("fuzz", d);
            }
            _ => {
                let d = regular_doc(&mut r, &id);
                let _ = api::update_entity_by_id(b"fuzz", id.as_bytes(), &d);
            }
        }
    }

    let got = hook::folds();
    assert!(
        got.incremental != 0,
        "writes should advance kept results, stats {got:?}"
    );
}

fn make2(r: &mut Rand, filters: &[&str], n: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let f = filters[r.intn(filters.len() as i64) as usize];
        out.push(
            ScriptGen {
                r,
                locals: Vec::new(),
            }
            .script(f),
        );
    }

    out
}

#[test]
fn results_stay_valid_across_writes_to_other_keys() {
    let _s = setup(true);
    api::create_entity_type(b"step").expect("create step");

    for i in 0..60 {
        write(
            "step",
            obj(vec![
                ("id", s(&format!("s{i:02}"))),
                ("program", s(&format!("p{}", i % 6))),
                ("status", s("vu")),
            ]),
        );
    }

    let script = r#"function postRead(ctx) {
  ctx.entity.ids = ctx.query("step", { program: { eq: ctx.entity.id } }).map(function (d) { return d.id + ":" + d.status }).join(",")
}"#;
    let run = |program: &str| -> String {
        hook::reset_output_memo();
        let out = entity(&format!(r#"{{"id":"{program}"}}"#));
        if let Err(err) = hook::apply_post_read_script("", script, &out, true) {
            panic!("{err}");
        }
        fmt_field(&out, "ids")
    };

    run("p0");
    let base = run("p0");

    let before = hook::query_cache();
    for i in 0..30 {
        let id = format!("s{:02}", 1 + i % 5 * 6);
        let _ = api::update_entity_by_id(
            b"step",
            id.as_bytes(),
            &obj(vec![("status", s(&format!("v{i}")))]),
        );
        let got = run("p0");
        assert_eq!(got, base, "a write to another program changed p0: {got}");
    }

    let after = hook::query_cache();
    assert_eq!(
        after.misses, before.misses,
        "writes to other keys must not rebuild the result: {before:?} -> {after:?}"
    );

    let _ = api::update_entity_by_id(b"step", b"s00", &obj(vec![("status", s("changed"))]));
    let got = run("p0");
    assert!(
        got.contains("s00:changed"),
        "a write to the key must be seen at once: {got}"
    );

    let _ = api::update_entity_by_id(b"step", b"s06", &obj(vec![("program", s("p1"))]));
    let got = run("p0");
    assert!(
        !got.contains("s06"),
        "a document that left the key must disappear: {got}"
    );
    let got = run("p1");
    assert!(
        got.contains("s06"),
        "a document that joined the key must appear: {got}"
    );

    api::delete_entity_by_id(b"step", b"s12");
    let got = run("p0");
    assert!(
        !got.contains("s12"),
        "a deleted document must disappear: {got}"
    );
}
