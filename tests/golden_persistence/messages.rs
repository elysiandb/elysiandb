use std::path::{Path, PathBuf};

use elysiandb::durable::{self, PAGE_HEADER_SIZE, filepath};
use elysiandb::gocompat::json::Value;
use elysiandb::journal::{self, Record};
use elysiandb::pagetree::Ref;

use super::common;
use super::support::{array, copy_tree, field, string, uint};
use super::tempdir::TempDir;

const CHECKED_HERE: [&str; 7] = [
    "ReadValue short read",
    "append after close",
    "close with unwritten data",
    "corrupt kv file",
    "page checksum mismatch",
    "missing page segment",
    "short page read",
];

const CHECKED_LATER: [&str; 6] = [
    "second open of a locked folder",
    "MANIFEST and MANIFEST.prev both unreadable",
    "live key 9x in MANIFEST",
    "unknown index name in MANIFEST",
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

    checked += page_messages(rows, tmp.path());

    assert_eq!(checked, CHECKED_HERE.len());
}

const CLEAN: &str = "persistence/folders/clean/before";

fn clean_root() -> (u64, u32) {
    let m = common::read_json(&format!("{CLEAN}/MANIFEST"));
    let entities = array(&m, "entities");
    let slots = field(&entities[0], "slots");
    (uint(slots, "id"), uint(slots, "size") as u32)
}

fn clean_pages(root: &Path, name: &str) -> PathBuf {
    let dir = filepath::join(root, name);
    copy_tree(&common::golden_path(&format!("{CLEAN}/pages")), &dir);
    dir
}

fn segment_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{e}"))
        .map(|e| e.unwrap_or_else(|e| panic!("{e}")).path())
        .filter(|p| p.extension().is_some_and(|x| x == "pgs"))
        .collect();
    out.sort();
    out
}

fn load_root(dir: &Path, id: u64, size: u32) -> durable::Error {
    let store = durable::open_page_store(None, dir, 1 << 30, 0).unwrap_or_else(|e| panic!("{e}"));
    let err = match store.load(&Ref::stored(id, size)) {
        Ok(_) => panic!("the root page of {} must not load", dir.display()),
        Err(err) => err,
    };
    store.close().unwrap_or_else(|e| panic!("{e}"));
    err
}

fn page_messages(rows: &[Value], root: &Path) -> usize {
    let (id, size) = clean_root();
    let (seg, off) = durable::split_page_id(id);
    let mut checked = 0;

    let flip = clean_pages(root, "page-checksum");
    let path = filepath::join(&flip, format!("{seg:06}.pgs"));
    let mut raw = std::fs::read(&path).unwrap_or_else(|e| panic!("{e}"));
    raw[(off + PAGE_HEADER_SIZE + 1) as usize] ^= 0x01;
    std::fs::write(&path, &raw).unwrap_or_else(|e| panic!("{e}"));
    let err = load_root(&flip, id, size);
    assert!(err.is_page_checksum(), "{err}");
    assert_eq!(
        message(rows, "page checksum mismatch"),
        format!("no readable checkpoint: MANIFEST: {err}")
    );
    checked += 1;

    let missing = clean_pages(root, "missing-pages");
    let files = segment_files(&missing);
    assert_eq!(files.len(), 2);
    for p in files {
        std::fs::remove_file(&p).unwrap_or_else(|e| panic!("{e}"));
    }
    let err = load_root(&missing, id, size);
    assert!(
        matches!(err, durable::Error::PageSegmentMissing(s) if s == seg),
        "{err}"
    );
    assert_eq!(
        message(rows, "missing page segment"),
        format!("no readable checkpoint: MANIFEST: {err}")
    );
    checked += 1;

    let short = clean_pages(root, "short-pages");
    let files = segment_files(&short);
    assert_eq!(files.len(), 2);
    for p in files {
        std::fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .and_then(|f| f.set_len(20))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let err = load_root(&short, id, size);
    assert!(err.fs_error().is_some_and(|e| e.is_eof()), "{err}");
    let (text, kinds) = wrapped(rows, "short page read");
    assert_eq!(kinds, vec!["end_of_file".to_string()]);
    assert_eq!(
        format!("no readable checkpoint: MANIFEST: {err}"),
        text.replace("<go:end_of_file>", "unexpected end of file")
    );
    assert_eq!(
        err.to_string(),
        format!("page {id:x}: unexpected end of file")
    );
    checked += 1;

    checked
}
