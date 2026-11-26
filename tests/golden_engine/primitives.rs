use crate::common;
use elysiandb::datastore::{self, Slot, SortInput};
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::time::Time;
use elysiandb::journal::Location;
use elysiandb::pagetree::{Token, Tree};

fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    common::str_field(row, key)
}

fn i(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer {key} in {row:?}"))
}

fn slot_json(sl: &Slot) -> String {
    format!(
        "{}/{}/{}/{}/{}/{}",
        sl.seq,
        common::hex(&sl.id),
        sl.lsn,
        sl.loc.segment,
        sl.loc.offset,
        sl.loc.length
    )
}

fn slot_of(v: &Value) -> Slot {
    Slot {
        seq: i(v, "seq") as u32,
        id: common::unhex(s(v, "id")),
        lsn: s(v, "lsn").parse().unwrap_or_else(|e| panic!("{e}")),
        loc: Location {
            segment: i(v, "segment") as u32,
            offset: i(v, "offset") as u32,
            length: i(v, "length") as u32,
        },
    }
}

#[test]
fn golden_primitives() {
    let rows = common::read_jsonl("engine/primitives.jsonl");
    assert_eq!(rows.len(), 112);

    let mut tree = Tree::new(None).with_token(Some(&Token::new()));
    let mut checked = 0;
    for row in &rows {
        let f = s(row, "fn");
        match f {
            "AppendNumber" => {
                let bits = u64::from_str_radix(s(row, "in"), 16).unwrap_or_default();
                let mut out = Vec::new();
                datastore::append_number(&mut out, f64::from_bits(bits));
                assert_eq!(common::hex(&out), s(row, "out"), "{row:?}");
            }
            "AppendTime" => {
                let input = row.get("in").unwrap_or_else(|| panic!("{row:?}"));
                let t = Time::from_unix(i(input, "unix"), i(input, "nanos"));
                let mut out = Vec::new();
                datastore::append_time(&mut out, &t);
                assert_eq!(common::hex(&out), s(row, "out"), "{row:?}");
            }
            "AppendEscaped" | "AppendTerminated" | "CompositePrefix" => {
                let input = common::unhex(s(row, "in"));
                let mut out = Vec::new();
                match f {
                    "AppendEscaped" => datastore::append_escaped(&mut out, &input),
                    "AppendTerminated" => datastore::append_terminated(&mut out, &input),
                    _ => out = datastore::composite_prefix(&input),
                }
                assert_eq!(common::hex(&out), s(row, "out"), "{row:?}");
            }
            "SortValueKey" => {
                let t = Time::from_unix(1_700_000_000, 5);
                let input = match s(row, "in") {
                    "nil" => SortInput::Nil,
                    "float64" | "float32" => SortInput::Number(2.5),
                    "int" => SortInput::Number(7.0),
                    "int32" => SortInput::Number(-7.0),
                    "int64" => SortInput::Number((1u64 << 40) as f64),
                    "time" => SortInput::Time(&t),
                    "string" => SortInput::String(b"a\x00b"),
                    "true" => SortInput::Bool(true),
                    "false" => SortInput::Bool(false),
                    "map" | "slice" | "uint" => SortInput::Other,
                    other => panic!("unknown SortValueKey input {other}"),
                };
                assert_eq!(
                    common::hex(&datastore::sort_value_key(input)),
                    s(row, "out"),
                    "{row:?}"
                );
            }
            "prefixEnd" => {
                let got = datastore::prefix_end(&common::unhex(s(row, "in")));
                match row.get("out") {
                    Some(Value::Null) => assert!(got.is_none(), "{row:?}"),
                    Some(Value::String(w)) => {
                        assert_eq!(common::hex(&got.unwrap_or_default()), *w, "{row:?}")
                    }
                    other => panic!("bad prefixEnd row {other:?}"),
                }
            }
            "seqKey" => {
                let seq = i(row, "in") as u32;
                assert_eq!(common::hex(&datastore::seq_key(seq)), s(row, "out"));
            }
            "postingKey" => {
                let input = row.get("in").unwrap_or_else(|| panic!("{row:?}"));
                let k = datastore::posting_key(i(input, "gram") as u32, i(input, "chunk") as u32);
                assert_eq!(common::hex(&k), s(row, "out"), "{row:?}");
            }
            "bitmapKey" => {
                let input = row.get("in").unwrap_or_else(|| panic!("{row:?}"));
                let k =
                    datastore::bitmap_key(s(input, "value").as_bytes(), i(input, "chunk") as u32);
                assert_eq!(common::hex(&k), s(row, "out"), "{row:?}");
            }
            "encodeSlot" => {
                let input = row.get("in").unwrap_or_else(|| panic!("{row:?}"));
                let sl = slot_of(input);
                let mut enc = Vec::new();
                datastore::encode_slot(&mut enc, &sl);
                assert_eq!(common::hex(&enc), s(row, "out"), "{row:?}");
                let d = datastore::decode_slot_value(sl.seq, &enc);
                let want = slot_of(row.get("decoded").unwrap_or_else(|| panic!("{row:?}")));
                assert_eq!(slot_json(&d), slot_json(&want), "{row:?}");
            }
            "decodeSlotValue" => {
                let d = datastore::decode_slot_value(9, &common::unhex(s(row, "in")));
                let want = slot_of(row.get("decoded").unwrap_or_else(|| panic!("{row:?}")));
                assert_eq!(slot_json(&d), slot_json(&want), "{row:?}");
            }
            "addIrregular" => {
                tree = datastore::add_irregular(tree, i(row, "in"));
                let got = tree.get(datastore::IRREGULAR_KEY);
                assert_eq!(
                    datastore::irregular_count(&tree),
                    i(row, "count"),
                    "{row:?}"
                );
                assert_eq!(
                    got.is_some(),
                    row.get("present").and_then(Value::as_bool).unwrap_or(false),
                    "{row:?}"
                );
                assert_eq!(
                    common::hex(&got.map(|g| g.to_vec()).unwrap_or_default()),
                    s(row, "stored"),
                    "{row:?}"
                );
            }
            other => panic!("unknown primitive {other}"),
        }
        checked += 1;
    }

    assert_eq!(checked, 112);
}
