use std::path::Path;
use std::sync::Arc;

use crate::common::{read_jsonl, str_field, unhex};
use crate::tempdir::TempDir;
use crate::{assert_manifest_entry, flag, int_of, security_config};
use elysiandb::globals;
use elysiandb::security::{self, SecurityError};

const ROWS: usize = 8;

#[test]
fn keyfile_rows_match_go() {
    assert_manifest_entry("keyfile.jsonl");
    let mut checked = 0;
    for row in &read_jsonl("security/keyfile.jsonl") {
        let dir = TempDir::new();
        let folder = dir.path().to_string_lossy().into_owned();
        globals::set_config(Some(Arc::new(security_config(&folder, ""))));

        let content = unhex(str_field(row, "content_hex"));
        let path = Path::new(&folder).join(security::KEY_FILENAME);
        if !content.is_empty() {
            std::fs::write(&path, &content).expect("write key file");
        }

        let result = security::create_key_file_or_get_key();
        if flag(row, "error") {
            let err = result.expect_err("expected an error");
            assert!(matches!(err, SecurityError::NoKeyFound(_)), "{row:?}");
            assert_eq!(
                err.to_string(),
                format!("read {folder}/{}: no key found", security::KEY_FILENAME),
                "{row:?}"
            );
        } else if flag(row, "generated") {
            let key = result.expect("generated key");
            assert_eq!(key.len() as i64, int_of(row, "length"), "{row:?}");
            assert!(
                key.bytes().all(|b| b"0123456789abcdef".contains(&b)),
                "{row:?}"
            );
            assert_eq!(std::fs::read(&path).expect("key file"), key.as_bytes());
        } else {
            assert_eq!(result.expect("key"), str_field(row, "key"), "{row:?}");
        }
        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
