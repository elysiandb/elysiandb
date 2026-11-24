use std::path::{Path, PathBuf};

use elysiandb::durable::{self, PAGE_HEADER_SIZE, filepath};
use elysiandb::gocompat::json::Value;
use elysiandb::journal::{self, Record};
use elysiandb::pagetree::Ref;

use super::common;
use super::folders::{folder_options, run_workload};
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

const CHECKED_BY_ENGINE: [&str; 6] = [
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
        let later = CHECKED_BY_ENGINE.contains(site);
        assert!(here != later, "{site} must be in exactly one list");
    }
    assert_eq!(sites.len(), CHECKED_HERE.len() + CHECKED_BY_ENGINE.len());
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

fn open_err(dir: &Path) -> String {
    match elysiandb::datastore::open(folder_options(dir)) {
        Ok(_) => panic!("{} must not open", dir.display()),
        Err(err) => err.to_string(),
    }
}

fn rewrite_manifest(dir: &Path, f: impl FnOnce(&mut elysiandb::gocompat::json::Map)) {
    let p = filepath::join(dir, "MANIFEST");
    let data = std::fs::read(&p).unwrap_or_else(|e| panic!("{e}"));
    let mut v = elysiandb::gocompat::json::unmarshal_value(&data).unwrap_or_else(|e| panic!("{e}"));
    let Value::Object(m) = &mut v else {
        panic!("MANIFEST is not an object");
    };
    f(m);
    let mut out =
        elysiandb::gocompat::json::marshal_indent(&v, "", "  ").unwrap_or_else(|e| panic!("{e}"));
    out.push(b'\n');
    std::fs::write(&p, out).unwrap_or_else(|e| panic!("{e}"));
}

fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[test]
fn engine_messages_match_go() {
    use elysiandb::gocompat::json::{Map, Value as J};

    let doc = common::read_json("persistence/messages.json");
    let rows = doc.as_array().expect("messages.json is an array");
    let tmp = TempDir::new();
    let mut checked = 0;

    let locked = filepath::join(tmp.path(), "locked");
    let e = elysiandb::datastore::open(folder_options(&locked)).unwrap_or_else(|e| panic!("{e}"));
    let err = open_err(&locked);
    assert_eq!(
        err,
        message(rows, "second open of a locked folder")
            .replace("<dir>", &locked.display().to_string())
    );
    e.close().unwrap_or_else(|e| panic!("{e}"));
    checked += 1;

    let base = filepath::join(tmp.path(), "base");
    run_workload(&base);

    let both = filepath::join(tmp.path(), "corrupt-both");
    copy_tree(&base, &both);
    for name in ["MANIFEST", "MANIFEST.prev"] {
        std::fs::write(filepath::join(&both, name), b"{\"format\":")
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let (text, kinds) = wrapped(rows, "MANIFEST and MANIFEST.prev both unreadable");
    assert_eq!(kinds, vec!["json".to_string()]);
    assert_eq!(
        open_err(&both),
        text.replace("<go:json>", "invalid JSON: unexpected end of input")
    );
    checked += 1;

    let bad_live = filepath::join(tmp.path(), "bad-live");
    copy_tree(&base, &bad_live);
    rewrite_manifest(&bad_live, |m| {
        let mut live = Map::new();
        live.insert("9x".into(), J::Number(1.0));
        m.insert("live".into(), J::Object(live));
    });
    remove(&filepath::join(&bad_live, "MANIFEST.prev"));
    assert_eq!(
        open_err(&bad_live),
        message(rows, "live key 9x in MANIFEST")
    );
    checked += 1;

    let unknown = filepath::join(tmp.path(), "unknown-index");
    copy_tree(&base, &unknown);
    rewrite_manifest(&unknown, |m| {
        let Some(J::Array(ents)) = m.get_mut("entities") else {
            panic!("no entities");
        };
        let J::Object(first) = &mut ents[0] else {
            panic!("bad entity");
        };
        let mut root = Map::new();
        root.insert("id".into(), J::Number(0.0));
        root.insert("size".into(), J::Number(0.0));
        let mut zz = Map::new();
        zz.insert("name".into(), J::String("zz".into()));
        zz.insert("root".into(), J::Object(root));
        match first.get_mut("indexes") {
            Some(J::Array(list)) => list.push(J::Object(zz)),
            _ => {
                first.insert("indexes".into(), J::Array(vec![J::Object(zz)]));
            }
        }
    });
    remove(&filepath::join(&unknown, "MANIFEST.prev"));
    assert_eq!(
        open_err(&unknown),
        message(rows, "unknown index name in MANIFEST")
    );
    checked += 1;

    let upgraded = filepath::join(tmp.path(), "format1-prev");
    copy_tree(&super::format1::fixture("pr-format1/format1"), &upgraded);
    let e = elysiandb::datastore::open(folder_options(&upgraded)).unwrap_or_else(|e| panic!("{e}"));
    e.abandon();
    let unreadable = filepath::join(tmp.path(), "format1-prev-unreadable");
    copy_tree(&upgraded, &unreadable);
    remove(&filepath::join(&upgraded, "MANIFEST"));
    let missing = |dir: &Path| {
        format!(
            "open {}: No such file or directory (os error 2)",
            filepath::join(dir, "checkpoint-0000000000000192.ckpt").display()
        )
    };
    let (text, kinds) = wrapped(
        rows,
        "format-1 MANIFEST.prev after upgrade, MANIFEST removed",
    );
    assert_eq!(kinds, vec!["file_system".to_string()]);
    assert_eq!(
        open_err(&upgraded),
        text.replace("<go:file_system>", &missing(&upgraded))
    );
    checked += 1;

    std::fs::write(filepath::join(&unreadable, "MANIFEST"), b"garbage")
        .unwrap_or_else(|e| panic!("{e}"));
    let (text, kinds) = wrapped(
        rows,
        "format-1 MANIFEST.prev after upgrade, MANIFEST unreadable",
    );
    assert_eq!(kinds, vec!["file_system".to_string()]);
    assert_eq!(
        open_err(&unreadable),
        text.replace("<go:file_system>", &missing(&unreadable))
    );
    checked += 1;

    assert_eq!(checked, CHECKED_BY_ENGINE.len());
}
