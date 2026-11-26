mod common;

use common::{hex, read_jsonl, str_field, unhex};
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::strings;

const REPLACEMENT: &[u8] = "\u{fffd}".as_bytes();

fn field_count(row: &Value) -> usize {
    row.as_object().expect("object row").len()
}

#[test]
fn strings_cases() {
    let rel = "gocompat/strings/cases.jsonl";
    let mut checked = 0;
    let mut transforms = 0;
    let mut folds = 0;
    for row in &read_jsonl(rel) {
        match str_field(row, "fn") {
            "Transform" => {
                assert_eq!(field_count(row), 7, "{row:?}");
                let input = unhex(str_field(row, "in"));
                assert_eq!(
                    hex(&strings::to_lower(&input)),
                    str_field(row, "to_lower"),
                    "ToLower {row:?}"
                );
                assert_eq!(
                    hex(&strings::to_upper(&input)),
                    str_field(row, "to_upper"),
                    "ToUpper {row:?}"
                );
                assert_eq!(
                    hex(strings::trim_space(&input)),
                    str_field(row, "trim_space"),
                    "TrimSpace {row:?}"
                );
                let want: Vec<&str> = row
                    .get("fields")
                    .and_then(Value::as_array)
                    .expect("fields array")
                    .iter()
                    .map(|v| v.as_str().expect("hex field"))
                    .collect();
                let got: Vec<String> = strings::fields(&input).into_iter().map(hex).collect();
                assert_eq!(got, want, "Fields {row:?}");
                assert_eq!(
                    hex(&strings::to_valid_utf8(&input, REPLACEMENT)),
                    str_field(row, "valid"),
                    "ToValidUTF8 {row:?}"
                );
                transforms += 1;
            }
            "EqualFold" => {
                assert_eq!(field_count(row), 4, "{row:?}");
                let a = unhex(str_field(row, "a"));
                let b = unhex(str_field(row, "b"));
                let equal = row
                    .get("equal")
                    .and_then(Value::as_bool)
                    .expect("equal flag");
                assert_eq!(strings::equal_fold(&a, &b), equal, "EqualFold {row:?}");
                folds += 1;
            }
            other => panic!("unknown strings case {other}"),
        }
        checked += 1;
    }
    assert!(transforms > 0 && folds > 0);
    assert_eq!(checked, common::lines(&common::read_golden(rel)).len());
}
