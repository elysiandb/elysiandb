mod common;

use common::{hex, read_jsonl, str_field, unhex};
use elysiandb::gocompat::base64::{CorruptInputError, RAW_URL_ENCODING, STD_ENCODING};
use elysiandb::gocompat::json::Value;

fn field_count(row: &Value) -> usize {
    row.as_object().expect("object row").len()
}

#[test]
fn base64_cases() {
    let rel = "gocompat/base64/cases.jsonl";
    let mut checked = 0;
    for row in &read_jsonl(rel) {
        let input = unhex(str_field(row, "in"));
        match str_field(row, "fn") {
            "encode" => {
                assert_eq!(field_count(row), 4, "{row:?}");
                let std = str_field(row, "std");
                let raw_url = str_field(row, "raw_url");
                assert_eq!(STD_ENCODING.encode_to_string(&input), std, "std {row:?}");
                assert_eq!(
                    RAW_URL_ENCODING.encode_to_string(&input),
                    raw_url,
                    "raw url {row:?}"
                );
                let mut appended = b"x".to_vec();
                STD_ENCODING.append_encode(&mut appended, &input);
                assert_eq!(&appended[1..], std.as_bytes(), "std append {row:?}");
                assert_eq!(
                    STD_ENCODING.decode_string(std.as_bytes()),
                    Ok(input.clone())
                );
                assert_eq!(
                    RAW_URL_ENCODING.decode_string(raw_url.as_bytes()),
                    Ok(input)
                );
            }
            "decode" => {
                let mut dbuf = vec![0; STD_ENCODING.decoded_len(input.len())];
                let (n, result) = STD_ENCODING.decode(&mut dbuf, &input);
                let mut appended = b"x".to_vec();
                let append_result = STD_ENCODING.append_decode(&mut appended, &input);
                assert_eq!(append_result, result, "AppendDecode {row:?}");
                assert_eq!(&appended[1..], &dbuf[..n], "AppendDecode {row:?}");
                if let Some(out) = row.get("out") {
                    assert_eq!(field_count(row), 3, "{row:?}");
                    let out = out.as_str().expect("hex output");
                    assert_eq!(result, Ok(()), "{row:?}");
                    assert_eq!(hex(&dbuf[..n]), out, "{row:?}");
                    let decoded = STD_ENCODING.decode_string(&input).expect("valid input");
                    assert_eq!(hex(&decoded), out, "{row:?}");
                } else if let Some(at) = row.get("corrupt_at") {
                    assert_eq!(field_count(row), 4, "{row:?}");
                    let at = usize::try_from(at.as_i64().expect("integer offset")).expect("offset");
                    assert_eq!(result, Err(CorruptInputError(at)), "{row:?}");
                    assert_eq!(hex(&dbuf[..n]), str_field(row, "partial"), "{row:?}");
                    let err = STD_ENCODING
                        .decode_string(&input)
                        .expect_err("corrupt input");
                    assert_eq!(err.offset(), at);
                    assert_eq!(err.to_string(), format!("invalid base64 at byte {at}"));
                } else {
                    panic!("decode row without an outcome the decoder can report: {row:?}");
                }
            }
            other => panic!("unknown base64 case {other}"),
        }
        checked += 1;
    }
    assert_eq!(checked, common::lines(&common::read_golden(rel)).len());
}
