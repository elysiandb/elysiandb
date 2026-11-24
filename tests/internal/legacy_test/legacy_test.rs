use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use elysiandb::configuration::{Config, StoreConfig};
use elysiandb::datastore;
use elysiandb::durable::filepath;
use elysiandb::globals;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{self, Base64Bytes, Map, Value};
use elysiandb::legacy;
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};

use super::tempdir::TempDir;

fn write_file(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
    let p = filepath::join(dir, name);
    let res = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&p)
        .and_then(|mut f| f.write_all(data));
    if let Err(e) = res {
        panic!("write {name}: {e}");
    }

    p
}

fn text(s: &str) -> Value {
    Value::String(s.to_string())
}

fn object(pairs: Vec<(&str, Value)>) -> Map {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn marshal<T: json::Encode + ?Sized>(v: &T) -> Vec<u8> {
    json::marshal(v).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn test_read_kv_file() {
    let dir = TempDir::new();
    let path = write_file(
        dir.path(),
        legacy::DATA_FILE,
        br#"{"foo":"YmFy","bin":"AQID"}"#,
    );

    let got = legacy::read_kv_file(&path).unwrap_or_else(|e| panic!("{e}"));

    let want = BTreeMap::from([
        (b"foo".to_vec(), Some(b"bar".to_vec())),
        (b"bin".to_vec(), Some(vec![0x01, 0x02, 0x03])),
    ]);
    if got != want {
        panic!("got={got:?} want={want:?}");
    }

    match legacy::read_kv_file(&write_file(dir.path(), "empty.json", b"")) {
        Ok(got) if got.is_empty() => {}
        other => panic!("an empty file is an empty map, got {other:?}"),
    }

    if legacy::read_kv_file(&filepath::join(dir.path(), "missing.json")).is_ok() {
        panic!("expected an error for a missing file");
    }

    if legacy::read_kv_file(&write_file(dir.path(), "bad.json", br#"{"foo":"#)).is_ok() {
        panic!("expected a JSON error");
    }
}

#[test]
fn test_read_expirations_file() {
    let dir = TempDir::new();
    let path = write_file(
        dir.path(),
        legacy::EXPIRATION_FILE,
        br#"{"1690000000":["a","b"],"1690000002":[],"not-a-number":["x"]}"#,
    );

    let got = legacy::read_expirations_file(&path).unwrap_or_else(|e| panic!("{e}"));

    let want = BTreeMap::from([
        (1690000000, vec![b"a".to_vec(), b"b".to_vec()]),
        (1690000002, Vec::new()),
    ]);
    if got != want {
        panic!("got={got:?} want={want:?}");
    }

    if legacy::read_expirations_file(&write_file(dir.path(), "bad.json", br#"{"169": ["ok"],"#))
        .is_ok()
    {
        panic!("expected a JSON error");
    }
}

fn log_lines(ops: Vec<Map>) -> Vec<u8> {
    let mut buf = Vec::new();
    for op in ops {
        buf.extend_from_slice(&marshal(&op));
        buf.push(b'\n');
    }

    buf
}

fn unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn b64(b: &[u8]) -> Value {
    Value::String(STD_ENCODING.encode_to_string(b))
}

fn doc_key(entity: &[u8], id: &[u8]) -> String {
    String::from_utf8(legacy::document_key(entity, id)).expect("utf-8")
}

#[test]
fn test_read_replays_the_recovery_logs_like024() {
    let dir = TempDir::new();
    let now = SystemTime::now();
    let expires_at = unix(now) + 3600;
    let big = vec![b'x'; 200_000];

    let kv: BTreeMap<String, Base64Bytes> = BTreeMap::from([
        ("old".to_string(), Base64Bytes(b"before".to_vec())),
        ("gone".to_string(), Base64Bytes(b"expired".to_vec())),
        (
            legacy::TYPES_LIST_KEY.to_string(),
            Base64Bytes(b"books".to_vec()),
        ),
        (
            String::from_utf8(legacy::id_index_key(b"books")).expect("utf-8"),
            Base64Bytes(b"b2\nghost\nb1\nb2".to_vec()),
        ),
        (
            format!("{}tmp", legacy::API_KEY_PREFIX),
            Base64Bytes(b"internal".to_vec()),
        ),
    ]);
    write_file(dir.path(), legacy::DATA_FILE, &marshal(&kv));
    write_file(dir.path(), legacy::EXPIRATION_FILE, br#"{"1":["gone"]}"#);

    let docs: BTreeMap<String, Map> = BTreeMap::from([
        (doc_key(b"books", b"b1"), object(vec![("id", text("b1"))])),
        (doc_key(b"books", b"b2"), object(vec![("id", text("b2"))])),
        (doc_key(b"authors", b"a1"), object(vec![("id", text("a1"))])),
    ]);
    write_file(dir.path(), legacy::JSON_DATA_FILE, &marshal(&docs));

    let mut store_log = log_lines(vec![
        object(vec![
            ("op", text("put")),
            ("key", text("session")),
            ("value", b64(b"short")),
            ("ttl", Value::Int(expires_at)),
        ]),
        object(vec![
            ("op", text("put")),
            ("key", text("late")),
            ("value", b64(b"x")),
            ("ttl", Value::Int(unix(now) - 10)),
        ]),
        object(vec![
            ("op", text("put")),
            ("key", text("big")),
            ("value", b64(&big)),
        ]),
        object(vec![("op", text("del")), ("key", text("old"))]),
    ]);
    store_log.extend_from_slice(b"{not json\n");
    write_file(dir.path(), legacy::STORE_RECOVERY_FILE, &store_log);

    let big_text = String::from_utf8(big.clone()).expect("utf-8");
    write_file(
        dir.path(),
        legacy::JSON_RECOVERY_FILE,
        &log_lines(vec![
            object(vec![
                ("op", text("put")),
                ("key", text(&doc_key(b"books", b"b3"))),
                (
                    "value",
                    Value::Object(object(vec![("id", text("b3")), ("text", text(&big_text))])),
                ),
            ]),
            object(vec![
                ("op", text("del")),
                ("key", text(&doc_key(b"books", b"b1"))),
            ]),
        ]),
    );

    let got = legacy::read(dir.path(), true, now).unwrap_or_else(|e| panic!("{e}"));

    if got.replayed != 5 {
        panic!("expected 5 replayed entries, got {}", got.replayed);
    }

    if got.kv.contains_key(b"old".as_slice()) {
        panic!("a logged delete removes the key");
    }
    if got.kv.contains_key(b"gone".as_slice()) {
        panic!("an expired key is dropped at load");
    }
    if got.kv.contains_key(b"late".as_slice()) {
        panic!("a logged key that expired since is not replayed");
    }
    let session = got.kv.get(b"session".as_slice()).cloned().flatten();
    let session_exp = got.expirations.get(b"session".as_slice()).copied();
    if session.as_deref() != Some(b"short".as_slice()) || session_exp != Some(expires_at) {
        panic!("the logged expiration is kept: {session:?} {session_exp:?}");
    }
    if got.kv.get(b"big".as_slice()).cloned().flatten().as_deref() != Some(big.as_slice()) {
        panic!("entries of any size are replayed");
    }

    if got.types.join(b",".as_slice()) != b"books,authors" {
        panic!("types are repaired in the 0.2.4 order, got {:?}", got.types);
    }

    let ids: Vec<Vec<u8>> = got
        .entities
        .get(b"books".as_slice())
        .map(|docs| docs.iter().map(|d| d.id.clone()).collect())
        .unwrap_or_default();
    if ids.join(b",".as_slice()) != b"b2,b3" {
        panic!("the id order is repaired like 0.2.4, got {ids:?}");
    }

    let without = legacy::read(dir.path(), false, now).unwrap_or_else(|e| panic!("{e}"));
    let books = without
        .entities
        .get(b"books".as_slice())
        .map_or(0, Vec::len);
    if without.replayed != 0 || books != 2 {
        panic!("without crash recovery the logs are ignored, as in 0.2.4: {without:?}");
    }
}

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}

struct CloseStorage;

impl Drop for CloseStorage {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn set_store(dir: &Path) {
    globals::set_config(Some(Arc::new(Config {
        store: StoreConfig {
            folder: dir.to_string_lossy().into_owned(),
            shards: 4,
            ..StoreConfig::default()
        },
        ..Config::default()
    })));
}

fn open_engine() -> Arc<datastore::Engine> {
    match storage::engine() {
        Some(e) => e,
        None => panic!("the storage is not open"),
    }
}

#[test]
fn test_migration_moves_the_legacy_files_and_keeps_the_data() {
    let _serial = serial();
    let dir = TempDir::new();
    let ttl_expires_at = unix(SystemTime::now()) + 600;
    let d = legacy::Data {
        kv: BTreeMap::from([
            (b"k".to_vec(), Some(b"v".to_vec())),
            (b"ttl".to_vec(), Some(b"t".to_vec())),
        ]),
        expirations: BTreeMap::from([(b"ttl".to_vec(), ttl_expires_at)]),
        types: vec![b"books".to_vec()],
        entities: BTreeMap::from([(
            b"books".to_vec(),
            vec![
                legacy::Document {
                    id: b"b2".to_vec(),
                    doc: Some(object(vec![("id", text("b2")), ("n", Value::Number(2.0))])),
                },
                legacy::Document {
                    id: b"b1".to_vec(),
                    doc: Some(object(vec![("id", text("b1"))])),
                },
            ],
        )]),
        replayed: 0,
    };
    if let Err(err) = legacy::write(dir.path(), &d) {
        panic!("{err}");
    }

    set_store(dir.path());
    storage::load_db();
    let _close = CloseStorage;

    let e = open_engine();
    match e.migration() {
        Some(report) if report.documents == 2 && report.kv_keys == 2 && report.entities == 1 => {}
        other => panic!("unexpected migration report {other:?}"),
    }

    for name in [
        legacy::DATA_FILE,
        legacy::JSON_DATA_FILE,
        legacy::EXPIRATION_FILE,
    ] {
        match std::fs::metadata(filepath::join(dir.path(), name)) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            _ => panic!("{name} should be moved aside"),
        }

        let kept = filepath::join(&filepath::join(dir.path(), datastore::LEGACY_FOLDER), name);
        match std::fs::metadata(&kept) {
            Ok(info) if info.permissions().mode() & 0o777 == 0o600 => {}
            other => panic!(
                "{name} should be kept in {} with mode 0600: {:?}",
                datastore::LEGACY_FOLDER,
                other.err()
            ),
        }
    }

    let snap = e.snapshot(b"books");
    let first_id = snap.at(0).map(|d| d.id().into_owned());
    if snap.len() != 2 || first_id.as_deref() != Some(b"b2".as_slice()) {
        panic!("documents keep their 0.2.4 order");
    }

    match storage::get_by_key(b"k") {
        Ok(v) if v == b"v" => {}
        other => panic!("kv keys are migrated: {other:?}"),
    }

    if storage::get_key_expiration(b"ttl") != Some(ttl_expires_at) {
        panic!("expirations are migrated");
    }

    drop(e);
    if let Err(err) = storage::close() {
        panic!("{err}");
    }

    storage::load_db();
    let e = open_engine();
    if e.migration().is_some() {
        panic!("a migrated folder is not migrated twice");
    }
    if e.snapshot(b"books").len() != 2 {
        panic!("the data survives a restart");
    }
}

#[test]
fn test_export_legacy_writes_a024_folder() {
    let _serial = serial();
    let dir = TempDir::new();
    set_store(dir.path());
    storage::load_db();
    let _close = CloseStorage;

    let e = open_engine();
    if let Err(err) = e.update(|tx| {
        tx.ensure_type(b"books");
        let _ = tx.put(
            b"books",
            b"b1",
            &object(vec![("id", text("b1")), ("title", text("Dune"))]),
        );
        tx.put(b"books", b"b0", &object(vec![("id", text("b0"))]))
    }) {
        panic!("{err}");
    }
    if let Err(err) = storage::put_key_value_with_ttl(b"k", b"v", 600) {
        panic!("{err}");
    }

    let out = TempDir::new();
    if let Err(err) = e.export_legacy(out.path()) {
        panic!("{err}");
    }

    let back = legacy::read(out.path(), false, SystemTime::now()).unwrap_or_else(|e| panic!("{e}"));

    let books = back.entities.get(b"books".as_slice());
    if back.types.join(b",".as_slice()) != b"books"
        || books.map_or(0, Vec::len) != 2
        || books.and_then(|b| b.first()).map(|d| d.id.as_slice()) != Some(b"b1".as_slice())
    {
        panic!("unexpected export {back:?}");
    }

    let value = back.kv.get(b"k".as_slice()).cloned().flatten();
    let expires_at = back.expirations.get(b"k".as_slice()).copied().unwrap_or(0);
    if value.as_deref() != Some(b"v".as_slice()) || expires_at == 0 {
        panic!("kv keys and expirations are exported");
    }
}
