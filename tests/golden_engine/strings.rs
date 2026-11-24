use std::collections::BTreeMap;

use crate::common;
use elysiandb::datastore::{self, KEY_LOWER};
use elysiandb::docpath;
use elysiandb::gocompat::fmt::{self as gofmt, Arg};
use elysiandb::gocompat::json::{self, Value};
use elysiandb::gocompat::strconv;

fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    common::str_field(row, key)
}

fn float_in(row: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(s(row, "in"), 16).unwrap_or_else(|e| panic!("{e}")))
}

fn block(start: u32) -> Vec<u8> {
    let mut out = String::new();
    let mut c = start;
    while c < start + 128 && c <= 0x10ffff {
        if let Some(ch) = char::from_u32(c) {
            out.push(ch);
        }
        c += 1;
    }
    out.into_bytes()
}

#[test]
fn golden_strings() {
    let rows = common::read_jsonl("engine/strings.jsonl");
    assert_eq!(rows.len(), 137);

    let mut blocks: BTreeMap<u32, String> = BTreeMap::new();
    let mut checked = 0;
    for row in &rows {
        match s(row, "fn") {
            "FormatFloat_f" => {
                assert_eq!(
                    strconv::format_float(float_in(row), b'f', -1, 64),
                    s(row, "out"),
                    "{row:?}"
                );
            }
            "Sprintf_v" => {
                let out = gofmt::sprintf("%v", &[Arg::Float(float_in(row))]);
                assert_eq!(String::from_utf8_lossy(&out), s(row, "out"), "{row:?}");
            }
            "ElementString" => {
                let out = datastore::element_string(&Value::Number(float_in(row)));
                assert_eq!(String::from_utf8_lossy(&out), s(row, "out"), "{row:?}");
            }
            "ElementString_json" => {
                let v = json::unmarshal_value(s(row, "in").as_bytes())
                    .unwrap_or_else(|e| panic!("{row:?}: {e}"));
                assert_eq!(
                    common::hex(&datastore::element_string(&v)),
                    s(row, "out"),
                    "{row:?}"
                );
            }
            "CompositePrefix_rule" => {}
            "CompositePrefix_block" => {
                let start = row.get("start").and_then(Value::as_i64).unwrap_or(-1) as u32;
                blocks.insert(start, s(row, "out").to_string());
            }
            "lowerKey" => {
                assert_eq!(
                    common::hex(&datastore::lower_key(&common::unhex(s(row, "in")))),
                    s(row, "out"),
                    "{row:?}"
                );
            }
            "PatternTrigrams" => {
                let got = datastore::pattern_trigrams(&common::unhex(s(row, "in")));
                let want: Vec<String> = row
                    .get("out")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|x| x.as_str().unwrap_or_default().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let got: Vec<String> = got.iter().map(|g| common::hex(g)).collect();
                assert_eq!(got, want, "{row:?}");
            }
            "MatchGlob" => {
                let got = docpath::match_glob(
                    &common::unhex(s(row, "pattern")),
                    &common::unhex(s(row, "subject")),
                );
                assert_eq!(
                    Some(got),
                    row.get("out").and_then(Value::as_bool),
                    "{row:?}"
                );
            }
            other => panic!("unknown strings fn {other}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 137);

    let mut listed = 0;
    let mut start = 0u32;
    while start <= 0x10ffff {
        if !(0xd800..0xe000).contains(&start) {
            let b = block(start);
            let got = common::hex(&datastore::composite_prefix(&b));
            match blocks.get(&start) {
                Some(want) => {
                    assert_eq!(&got, want, "block {start:#x}");
                    listed += 1;
                }
                None => {
                    let mut unchanged = vec![KEY_LOWER];
                    datastore::append_terminated(&mut unchanged, &b);
                    assert_eq!(got, common::hex(&unchanged), "block {start:#x}");
                }
            }
        }
        start += 128;
    }
    assert_eq!(listed, blocks.len());
}
