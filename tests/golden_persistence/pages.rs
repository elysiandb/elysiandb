use std::collections::BTreeSet;

use crate::common;
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::strconv;
use elysiandb::pagetree;

const REL: &str = "persistence/pages/decode.json";

fn fields(row: &Value) -> BTreeSet<String> {
    row.as_object().expect("object").keys().cloned().collect()
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key)
        .unwrap_or_else(|| panic!("missing field {key} in {row:?}"))
}

fn dec64(v: &Value) -> u64 {
    let s = v
        .as_str()
        .unwrap_or_else(|| panic!("not a decimal string: {v:?}"));
    strconv::parse_uint(s.as_bytes(), 10, 64).unwrap_or_else(|e| panic!("bad integer {s}: {e}"))
}

fn int(v: &Value) -> u64 {
    v.as_u64()
        .unwrap_or_else(|| panic!("not an integer: {v:?}"))
}

#[test]
fn golden_page_decode() {
    let rows = common::read_json(REL);
    let rows = rows.as_array().expect("array of cases");

    let mut names = BTreeSet::new();
    let mut checked = 0;
    for row in rows {
        let name = common::str_field(row, "name");
        assert!(names.insert(name.to_string()), "duplicate case {name}");
        let bytes = common::unhex(common::str_field(row, "hex"));

        match pagetree::decode(&bytes) {
            Err(err) => {
                assert_eq!(
                    fields(row),
                    set(&["error", "hex", "name"]),
                    "{name}: fields"
                );
                assert_eq!(common::str_field(row, "error"), err.to_string(), "{name}");
            }
            Ok(p) => {
                assert_eq!(fields(row), set(&["hex", "name", "page"]), "{name}: fields");
                let page = field(row, "page");
                let mut expected = set(&["leaf", "len", "total"]);

                assert_eq!(
                    field(page, "leaf").as_bool(),
                    Some(p.leaf()),
                    "{name}: leaf"
                );
                assert_eq!(int(field(page, "len")), p.len() as u64, "{name}: len");
                assert_eq!(dec64(field(page, "total")), p.total(), "{name}: total");

                if let Ok(enc) = pagetree::encode(Vec::new(), &p) {
                    expected.insert("reencoded".to_string());
                    assert_eq!(
                        common::str_field(page, "reencoded"),
                        common::hex(&enc),
                        "{name}: reencoded"
                    );
                }

                if !p.kids().is_empty() {
                    expected.insert("kids".to_string());
                    let kids = field(page, "kids").as_array().expect("kids");
                    assert_eq!(kids.len(), p.kids().len(), "{name}: kid count");
                    for (w, k) in kids.iter().zip(p.kids()) {
                        assert_eq!(fields(w), set(&["id", "size"]), "{name}: kid fields");
                        assert_eq!(dec64(field(w, "id")), k.id(), "{name}: kid id");
                        assert_eq!(
                            int(field(w, "size")),
                            u64::from(k.size()),
                            "{name}: kid size"
                        );
                    }
                }

                assert_eq!(fields(page), expected, "{name}: page fields");
            }
        }
        checked += 1;
    }

    assert_eq!(checked, rows.len());
    assert_eq!(checked, 19);
}
