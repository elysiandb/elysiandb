mod common;

use common::{hex, read_jsonl, str_field, unhex};
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::utf8::{self, Rune};
use elysiandb::gocompat::utf16;

fn int(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

fn rune(row: &Value, key: &str) -> Rune {
    Rune::try_from(int(row, key)).unwrap_or_else(|e| panic!("{key} in {row:?}: {e}"))
}

fn size(row: &Value, key: &str) -> usize {
    usize::try_from(int(row, key)).unwrap_or_else(|e| panic!("{key} in {row:?}: {e}"))
}

fn boolean(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing bool field {key} in {row:?}"))
}

fn array<'a>(row: &'a Value, key: &str) -> &'a Vec<Value> {
    row.get(key)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing array field {key} in {row:?}"))
}

fn runes(row: &Value, key: &str) -> Vec<Rune> {
    array(row, key)
        .iter()
        .map(|v| Rune::try_from(v.as_i64().expect("integer element")).expect("rune element"))
        .collect()
}

fn units(row: &Value, key: &str) -> Vec<u16> {
    array(row, key)
        .iter()
        .map(|v| u16::try_from(v.as_i64().expect("integer element")).expect("utf-16 element"))
        .collect()
}

fn field_count(row: &Value) -> usize {
    row.as_object().expect("object row").len()
}

fn line_count(rel: &str) -> usize {
    common::lines(&common::read_golden(rel)).len()
}

#[test]
fn utf8_decode() {
    let rel = "gocompat/utf8/decode.jsonl";
    let mut checked = 0;
    for row in &read_jsonl(rel) {
        assert_eq!(field_count(row), 8, "{row:?}");
        let input = unhex(str_field(row, "in"));
        assert_eq!(
            utf8::decode_rune(&input),
            (rune(row, "rune"), size(row, "size")),
            "DecodeRune {row:?}"
        );
        assert_eq!(
            utf8::decode_last_rune(&input),
            (rune(row, "last_rune"), size(row, "last_size")),
            "DecodeLastRune {row:?}"
        );
        assert_eq!(utf8::valid(&input), boolean(row, "valid"), "Valid {row:?}");
        assert_eq!(
            utf8::full_rune(&input),
            boolean(row, "full"),
            "FullRune {row:?}"
        );
        assert_eq!(
            utf8::rune_count(&input),
            size(row, "count"),
            "RuneCount {row:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, line_count(rel));
}

#[test]
fn utf8_encode() {
    let rel = "gocompat/utf8/encode.jsonl";
    let mut checked = 0;
    for row in &read_jsonl(rel) {
        assert_eq!(field_count(row), 4, "{row:?}");
        let r = rune(row, "rune");
        let mut appended = vec![0x41];
        utf8::append_rune(&mut appended, r);
        assert_eq!(appended[0], 0x41);
        assert_eq!(
            hex(&appended[1..]),
            str_field(row, "append"),
            "AppendRune {row:?}"
        );
        let mut buf = [0u8; utf8::UTF_MAX];
        let n = utf8::encode_rune(&mut buf, r);
        assert_eq!(
            hex(&buf[..n]),
            str_field(row, "append"),
            "EncodeRune {row:?}"
        );
        let len = utf8::rune_len(r).map_or(-1, |n| i64::try_from(n).expect("small length"));
        assert_eq!(len, int(row, "len"), "RuneLen {row:?}");
        assert_eq!(
            utf8::valid_rune(r),
            boolean(row, "valid"),
            "ValidRune {row:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, line_count(rel));
}

#[test]
fn utf16_cases() {
    let rel = "gocompat/utf16/cases.jsonl";
    let mut checked = 0;
    for row in &read_jsonl(rel) {
        match str_field(row, "fn") {
            "EncodeRune" => {
                assert_eq!(field_count(row), 5, "{row:?}");
                let r = rune(row, "rune");
                assert_eq!(
                    utf16::encode_rune(r),
                    (rune(row, "r1"), rune(row, "r2")),
                    "EncodeRune {row:?}"
                );
                assert_eq!(
                    utf16::is_surrogate(r),
                    boolean(row, "surrogate"),
                    "IsSurrogate {row:?}"
                );
            }
            "DecodeRune" => {
                assert_eq!(field_count(row), 4, "{row:?}");
                assert_eq!(
                    utf16::decode_rune(rune(row, "r1"), rune(row, "r2")),
                    rune(row, "rune"),
                    "DecodeRune {row:?}"
                );
            }
            "Decode" => {
                assert_eq!(field_count(row), 3, "{row:?}");
                assert_eq!(
                    utf16::decode(&units(row, "in")),
                    runes(row, "runes"),
                    "Decode {row:?}"
                );
            }
            "Encode" => {
                assert_eq!(field_count(row), 3, "{row:?}");
                let input = runes(row, "runes");
                let want = units(row, "out");
                assert_eq!(utf16::encode(&input), want, "Encode {row:?}");
                let mut appended = Vec::new();
                for &r in &input {
                    utf16::append_rune(&mut appended, r);
                }
                assert_eq!(appended, want, "AppendRune {row:?}");
            }
            other => panic!("unknown utf16 case {other}"),
        }
        checked += 1;
    }
    assert_eq!(checked, line_count(rel));
}
