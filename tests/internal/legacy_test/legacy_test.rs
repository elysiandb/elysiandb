use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use elysiandb::durable::filepath;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{self, Base64Bytes, Map, Value};
use elysiandb::legacy;

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
