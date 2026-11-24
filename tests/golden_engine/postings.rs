use std::collections::BTreeMap;

use crate::common;
use elysiandb::datastore::{self, Bitset, IndexOp, OpKind, Posting};
use elysiandb::gocompat::json::Value;
use elysiandb::pagetree::{Token, Tree};

fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    common::str_field(row, key)
}

fn ints(v: Option<&Value>) -> Vec<i64> {
    match v {
        Some(Value::Array(a)) => a.iter().map(|x| x.as_i64().unwrap_or(-1)).collect(),
        _ => Vec::new(),
    }
}

fn add(c: &[u8], off: u16) -> Vec<u8> {
    match datastore::posting_add(c, off) {
        Posting::Same => c.to_vec(),
        Posting::Empty => Vec::new(),
        Posting::New(n) => n,
    }
}

fn mk(offs: &[usize]) -> Vec<u8> {
    let mut c = Vec::new();
    for &o in offs {
        c = add(&c, o as u16);
    }
    c
}

fn kind(n: i64) -> OpKind {
    match n {
        0 => OpKind::Put,
        1 => OpKind::Delete,
        2 => OpKind::BitmapAdd,
        3 => OpKind::BitmapRemove,
        4 => OpKind::PostingAdd,
        5 => OpKind::IrregularUp,
        6 => OpKind::IrregularDown,
        _ => panic!("bad op kind {n}"),
    }
}

#[test]
fn golden_postings() {
    let rows = common::read_jsonl("engine/postings.jsonl");
    assert_eq!(rows.len(), 58);

    let big: Vec<usize> = (0..400).map(|i| i * 3).collect();
    let mut containers: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
    containers.insert("small", mk(&[1, 2, 3, 500]));
    containers.insert("other", mk(&[2, 3, 4, 1000]));
    containers.insert("big", mk(&big));
    containers.insert("empty", Vec::new());

    let mut checked = 0;
    for row in &rows {
        let script = s(row, "script");
        match script {
            "bitset" => {
                let names = |k: &str| -> Vec<String> {
                    row.get(k)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .map(|x| x.as_str().unwrap_or_default().to_string())
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let a_names = names("a");
                let b_names = names("b");
                let mut a = Bitset::new(1 << 17);
                a.add_container(0, &containers[a_names[0].as_str()]);
                a.add_container(1, &containers[a_names[1].as_str()]);
                let mut b = Bitset::new(1 << 17);
                b.add_container(0, &containers[b_names[0].as_str()]);
                b.add_container(1, &containers[b_names[1].as_str()]);
                match s(row, "op") {
                    "And" => a.and(&b),
                    "AndNot" => a.and_not(&b),
                    "Or" => a.or(&b),
                    other => panic!("bad op {other}"),
                }
                let mut bits = Vec::new();
                a.for_each(|i| {
                    bits.push(i64::from(i));
                    true
                });
                assert_eq!(bits, ints(row.get("bits")), "{row:?}");
                assert_eq!(
                    a.count() as i64,
                    row.get("count").and_then(Value::as_i64).unwrap_or(-1)
                );
            }
            "container" => {
                assert_eq!(common::hex(&containers[s(row, "name")]), s(row, "hex"));
            }
            _ if script.starts_with("applyOps-") => {
                let mut ops: Vec<IndexOp> = row
                    .get("ops")
                    .and_then(Value::as_array)
                    .unwrap_or_else(|| panic!("{row:?}"))
                    .iter()
                    .map(|o| IndexOp {
                        key: common::unhex(s(o, "key")),
                        seq: o.get("seq").and_then(Value::as_i64).unwrap_or(0) as u32,
                        kind: kind(o.get("kind").and_then(Value::as_i64).unwrap_or(-1)),
                    })
                    .collect();
                let tree =
                    datastore::apply_ops(Tree::new(None).with_token(Some(&Token::new())), &mut ops);
                let mut dump: Vec<(String, String)> = Vec::new();
                tree.scan(|k, v| {
                    dump.push((common::hex(k), common::hex(v)));
                    true
                });
                let want: Vec<(String, String)> = match row.get("dump") {
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|p| {
                            (
                                p.index(0)
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                p.index(1)
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                            )
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                assert_eq!(dump, want, "{row:?}");
            }
            _ => {
                let ops = row
                    .get("ops")
                    .and_then(Value::as_array)
                    .unwrap_or_else(|| panic!("{row:?}"));
                let steps = row
                    .get("steps")
                    .and_then(Value::as_array)
                    .unwrap_or_else(|| panic!("{row:?}"));
                assert_eq!(ops.len(), steps.len());
                let mut c: Vec<u8> = Vec::new();
                for (op, step) in ops.iter().zip(steps) {
                    let sign = op.index(0).and_then(Value::as_i64).unwrap_or(0);
                    let off = op.index(1).and_then(Value::as_i64).unwrap_or(0) as u16;
                    c = if sign > 0 {
                        add(&c, off)
                    } else {
                        match datastore::posting_remove(&c, off) {
                            Posting::Same => c.clone(),
                            Posting::Empty => Vec::new(),
                            Posting::New(n) => n,
                        }
                    };
                    assert_eq!(
                        common::hex(&c),
                        step.as_str().unwrap_or_default(),
                        "{script}"
                    );
                }
                assert_eq!(
                    datastore::posting_count(&c) as i64,
                    row.get("count").and_then(Value::as_i64).unwrap_or(-1),
                    "{script}"
                );
                let mut each = Vec::new();
                datastore::posting_each(&c, 1 << datastore::POSTING_CHUNK_BITS, |seq| {
                    each.push(i64::from(seq))
                });
                assert_eq!(each, ints(row.get("each_base_chunk1")), "{script}");
            }
        }
        checked += 1;
    }

    assert_eq!(checked, 58);
}
