use std::path::Path;

use crate::common::{read_jsonl, str_field, unhex};
use crate::tempdir::TempDir;
use crate::{assert_manifest_entry, flag};
use elysiandb::gocompat::json;
use elysiandb::security;

const ROWS: usize = 15;

fn expected_error(file: &[u8]) -> &'static str {
    match file {
        br#"{"sessions":"# | b"" | b"   " => "invalid JSON: unexpected end of input",
        b"[]" => "invalid JSON: the value must be an object, got an array",
        br#"{"sessions":{}}"# => r#"invalid JSON: "sessions" must be an array, got an object"#,
        br#"{"sessions":[{"expires_at":"5"}]}"# => {
            r#"invalid JSON: "sessions[0].expires_at" must be an integer, got a string"#
        }
        br#"{"sessions":[{"expires_at":1.5}]}"# => {
            r#"invalid JSON: "sessions[0].expires_at" must be an integer, got a number"#
        }
        other => panic!(
            "no expected message for {:?}",
            String::from_utf8_lossy(other)
        ),
    }
}

#[test]
fn sessions_decode_rows_match_go() {
    assert_manifest_entry("sessions_decode.jsonl");
    let mut checked = 0;
    for row in &read_jsonl("security/sessions_decode.jsonl") {
        let dir = TempDir::new();
        let path = Path::new(dir.path()).join(security::SESSIONS_FILENAME);
        let file = unhex(str_field(row, "file_hex"));
        std::fs::write(&path, &file).expect("write sessions file");

        let result = security::read_sessions_file(&path.to_string_lossy());
        if flag(row, "error") {
            let err = result.expect_err("expected a decode error");
            assert_eq!(err.to_string(), expected_error(&file), "{row:?}");
        } else {
            let sf = result.unwrap_or_else(|e| panic!("{row:?}: {e}"));
            let out = json::marshal(&sf).expect("marshal");
            assert_eq!(
                String::from_utf8_lossy(&out),
                str_field(row, "decoded"),
                "{row:?}"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
