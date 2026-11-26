use elysiandb::durable::{self, KvRecord, filepath};
use elysiandb::gocompat::json::Value;

use super::common;
use super::support::{array, dec_i64, dec_u64, hex_bytes, string, uint};
use super::tempdir::TempDir;

fn records(rows: &[Value]) -> Vec<KvRecord> {
    rows.iter()
        .map(|r| KvRecord {
            key: hex_bytes(r, "key_hex"),
            lsn: dec_u64(r, "lsn"),
            expires_at: dec_i64(r, "expires_at"),
            segment: uint(r, "segment") as u32,
            offset: uint(r, "offset") as u32,
            length: uint(r, "length") as u32,
        })
        .collect()
}

#[test]
fn kv_files_match_go() {
    let cases = common::read_json("persistence/kvfile/cases.json");
    let cases = cases.as_array().expect("cases.json is an array");
    let tmp = TempDir::new();

    let mut written = 0;
    let mut read = 0;
    let mut rejected = 0;
    for row in cases {
        let file = string(row, "file");
        let golden = common::golden_path(&format!("persistence/kvfile/{file}"));

        if row.get("written").is_some() {
            let recs = records(array(row, "written"));
            let out = filepath::join(tmp.path(), file);
            if let Err(e) = durable::write_kv_file(&durable::OS, &out, &recs) {
                panic!("{file}: {e}");
            }
            let ours = std::fs::read(&out).expect("written file");
            let theirs = std::fs::read(&golden).expect("golden file");
            assert_eq!(common::hex(&ours), common::hex(&theirs), "{file}");
            assert!(!filepath::join(tmp.path(), format!("{file}.tmp")).exists());
            written += 1;
        }

        match durable::read_kv_file(&durable::OS, &golden) {
            Ok(got) => {
                let want = records(array(row, "read"));
                assert_eq!(got, want, "{file}");
                assert!(row.get("error").is_none(), "{file}");
                read += 1;
            }
            Err(e) => {
                assert!(e.is_corrupt_file(), "{file}: {e}");
                assert_eq!(e.to_string(), string(row, "error"), "{file}");
                assert!(row.get("read").is_none(), "{file}");
                rejected += 1;
            }
        }
    }

    assert_eq!(written, 5);
    assert_eq!(read + rejected, cases.len());
    assert_eq!(cases.len(), 12);
    assert_eq!(rejected, 7);
}
