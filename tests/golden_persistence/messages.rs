use elysiandb::durable::{self, filepath};
use elysiandb::gocompat::json::Value;
use elysiandb::journal::{self, Record};

use super::common;
use super::support::{array, field, string};
use super::tempdir::TempDir;

const CHECKED_HERE: [&str; 4] = [
    "ReadValue short read",
    "append after close",
    "close with unwritten data",
    "corrupt kv file",
];

const CHECKED_LATER: [&str; 9] = [
    "second open of a locked folder",
    "MANIFEST and MANIFEST.prev both unreadable",
    "live key 9x in MANIFEST",
    "unknown index name in MANIFEST",
    "page checksum mismatch",
    "missing page segment",
    "short page read",
    "format-1 MANIFEST.prev after upgrade, MANIFEST removed",
    "format-1 MANIFEST.prev after upgrade, MANIFEST unreadable",
];

fn row<'a>(rows: &'a [Value], site: &str) -> &'a Value {
    rows.iter()
        .find(|r| string(r, "site") == site)
        .unwrap_or_else(|| panic!("missing site {site}"))
}

fn message(rows: &[Value], site: &str) -> String {
    let err = field(row(rows, site), "error");
    assert!(err.get("text").is_none(), "{site}: {err:?}");
    string(err, "message").to_string()
}

fn wrapped(rows: &[Value], site: &str) -> (String, Vec<String>) {
    let err = field(row(rows, site), "error");
    let kinds = array(err, "wrapped")
        .iter()
        .map(|k| k.as_str().expect("kind").to_string())
        .collect();
    (string(err, "text").to_string(), kinds)
}

#[test]
fn log_and_kv_file_messages_match_go() {
    let doc = common::read_json("persistence/messages.json");
    let rows = doc.as_array().expect("messages.json is an array");

    let sites: Vec<&str> = rows.iter().map(|r| string(r, "site")).collect();
    for site in &sites {
        let here = CHECKED_HERE.contains(site);
        let later = CHECKED_LATER.contains(site);
        assert!(here != later, "{site} must be in exactly one list");
    }
    assert_eq!(sites.len(), CHECKED_HERE.len() + CHECKED_LATER.len());
    assert_eq!(rows.len(), 13);
    assert!(!sites.contains(&"journal close"));

    let tmp = TempDir::new();
    let jdir = filepath::join(tmp.path(), "journal");
    let j = journal::open(
        &jdir,
        journal::Options {
            mode: journal::Mode::Sync,
            segment_size: 4096,
            ..journal::Options::default()
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    j.start();
    let mut recs = vec![Record {
        kind: journal::KIND_KV_PUT,
        key: b"k".to_vec(),
        value: b"value".to_vec(),
        ..Record::default()
    }];
    let ticket = j.append(&mut recs).unwrap_or_else(|e| panic!("{e}"));
    ticket.wait().unwrap_or_else(|e| panic!("{e}"));
    let segment = filepath::join(&jdir, journal::segment_name(recs[0].loc.segment));
    std::fs::OpenOptions::new()
        .write(true)
        .open(&segment)
        .and_then(|f| f.set_len(u64::from(recs[0].loc.offset) + 2))
        .unwrap_or_else(|e| panic!("{e}"));

    let mut checked = 0;

    let err = j
        .read_value(recs[0].loc)
        .expect_err("a short read must fail");
    assert_eq!(
        wrapped(rows, "ReadValue short read"),
        (
            "<go:end_of_file>".to_string(),
            vec!["end_of_file".to_string()]
        )
    );
    assert_eq!(err.to_string(), "unexpected end of file");
    checked += 1;

    j.close()
        .unwrap_or_else(|e| panic!("the journal closes without error, like Go: {e}"));

    let err = j.append(&mut recs).expect_err("append after close");
    assert_eq!(err.to_string(), message(rows, "append after close"));
    checked += 1;

    let bg_dir = filepath::join(tmp.path(), "journal-bg");
    let bg = journal::open(
        &bg_dir,
        journal::Options {
            mode: journal::Mode::Background,
            segment_size: 4096,
            ..journal::Options::default()
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let mut recs = vec![Record {
        kind: journal::KIND_KV_PUT,
        key: b"k".to_vec(),
        value: b"v".to_vec(),
        ..Record::default()
    }];
    bg.append(&mut recs).unwrap_or_else(|e| panic!("{e}"));
    let err = bg.close().expect_err("close with unwritten data");
    assert_eq!(err.to_string(), message(rows, "close with unwritten data"));
    checked += 1;

    let kv = filepath::join(tmp.path(), "kv-0000000000000390.ckpt");
    let mut data = common::read_golden("persistence/folders/clean/before/kv-0000000000000390.ckpt");
    let mid = data.len() / 2;
    data[mid] ^= 0x20;
    std::fs::write(&kv, &data).unwrap_or_else(|e| panic!("{e}"));
    let err = durable::read_kv_file(&durable::OS, &kv).expect_err("a corrupt kv file");
    assert!(err.is_corrupt_file());
    let full = message(rows, "corrupt kv file");
    assert_eq!(
        full,
        format!("no readable checkpoint: MANIFEST: cannot read kv-0000000000000390.ckpt: {err}")
    );
    checked += 1;

    assert_eq!(checked, CHECKED_HERE.len());
}
