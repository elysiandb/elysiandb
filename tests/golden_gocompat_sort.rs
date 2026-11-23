mod common;

use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::gocompat::sort;

fn ints(v: &Value) -> Vec<i64> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_i64().expect("integer"))
        .collect()
}

fn text(v: &Value) -> String {
    match v {
        Value::Number(n) => n.as_str().to_string(),
        Value::String(s) => s.clone(),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn slice_and_slice_stable_match_go() {
    let rel = "gocompat/sort/slice.jsonl";
    let rows = common::read_jsonl(rel);
    assert_eq!(rows.len(), common::lines(&common::read_golden(rel)).len());
    for row in &rows {
        let keys = ints(row.get("keys").expect("keys"));
        let mut a: Vec<(i64, i64)> = keys
            .iter()
            .enumerate()
            .map(|(i, &k)| (k, i as i64))
            .collect();
        let mut b = a.clone();
        sort::slice(&mut a, |x, y| x.0 < y.0);
        sort::slice_stable(&mut b, |x, y| x.0 < y.0);
        let perm: Vec<i64> = a.iter().map(|x| x.1).collect();
        let stable: Vec<i64> = b.iter().map(|x| x.1).collect();
        assert_eq!(
            perm,
            ints(row.get("slice").expect("slice")),
            "slice of {keys:?}"
        );
        assert_eq!(
            stable,
            ints(row.get("slice_stable").expect("slice_stable")),
            "stable of {keys:?}"
        );
    }
}

#[test]
fn rand_sources_match_go() {
    let rel = "gocompat/rand/sources.jsonl";
    let rows = common::read_jsonl(rel);
    assert_eq!(rows.len(), common::lines(&common::read_golden(rel)).len());
    let mut ops_checked = 0;
    for row in &rows {
        let seed: i64 = common::str_field(row, "seed").parse().expect("seed");
        let mut r = Rand::new(rand::new_source(seed));
        for (i, v) in row
            .get("int63")
            .and_then(Value::as_array)
            .expect("int63")
            .iter()
            .enumerate()
        {
            assert_eq!(r.int63().to_string(), text(v), "seed {seed} int63 #{i}");
        }
        let mut r = Rand::new(rand::new_source(seed));
        for op in row.get("ops").and_then(Value::as_array).expect("ops") {
            let op = op.as_array().expect("op");
            let name = op[0].as_str().expect("op name");
            match name {
                "Intn" => {
                    let n = op[1].as_i64().expect("n");
                    assert_eq!(
                        r.intn(n),
                        op[2].as_i64().expect("v"),
                        "seed {seed} Intn({n})"
                    );
                }
                "Float64" => {
                    let bits = u64::from_str_radix(op[1].as_str().expect("bits"), 16).expect("hex");
                    assert_eq!(r.float64().to_bits(), bits, "seed {seed} Float64");
                }
                "Int63n" => {
                    let n: i64 = text(&op[1]).parse().expect("n");
                    assert_eq!(
                        r.int63n(n).to_string(),
                        text(&op[2]),
                        "seed {seed} Int63n({n})"
                    );
                }
                "Int31n" => {
                    let n = op[1].as_i64().expect("n") as i32;
                    assert_eq!(
                        i64::from(r.int31n(n)),
                        op[2].as_i64().expect("v"),
                        "seed {seed} Int31n({n})"
                    );
                }
                "Perm" => {
                    let n = op[1].as_i64().expect("n") as usize;
                    assert_eq!(r.perm(n), ints(&op[2]), "seed {seed} Perm({n})");
                }
                "Uint64" => {
                    assert_eq!(r.uint64().to_string(), text(&op[1]), "seed {seed} Uint64");
                }
                other => panic!("unknown op {other}"),
            }
            ops_checked += 1;
        }
    }
    assert_eq!(ops_checked, rows.len() * 600);
}
