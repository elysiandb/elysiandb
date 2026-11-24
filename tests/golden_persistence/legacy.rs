use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, UNIX_EPOCH};

use elysiandb::gocompat::json::{self, ErrorKind, Kind, Map, Value};
use elysiandb::legacy::{self, Data, Document};

use super::common;
use super::support::{array, assert_same_tree, copy_tree, dec_i64, field, int, string};
use super::tempdir::TempDir;

const READ: &str = "persistence/legacy/read";
const WRITE: &str = "persistence/legacy/write";

fn case_names(rel: &str) -> Vec<String> {
    let dir = common::golden_path(rel);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|e| {
            e.unwrap_or_else(|e| panic!("{e}"))
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn go_kind(value: &str) -> Kind {
    match value {
        "array" => Kind::Array,
        "object" => Kind::Object,
        "string" => Kind::String,
        "number" => Kind::Number,
        "bool" => Kind::Boolean,
        other => panic!("unexpected value kind {other}"),
    }
}

fn check_error(name: &str, input: &[u8], err: &legacy::Error, want: &Value) {
    let file = string(want, "file");
    let wrapped = string(want, "wrapped");
    let offset = int(want, "offset");
    let msg = err.to_string();
    let prefix = format!("{file}: ");
    assert!(msg.starts_with(&prefix), "{name}: {msg}");
    let jerr = err
        .json()
        .unwrap_or_else(|| panic!("{name}: not a JSON error: {msg}"));

    match wrapped {
        "json_syntax" => {
            let syntax = jerr
                .syntax()
                .unwrap_or_else(|| panic!("{name}: not a syntax error: {msg}"));
            assert_eq!(syntax.offset, offset, "{name}: offset");
            assert_eq!(
                offset as usize,
                input.len(),
                "{name}: only the end of input is covered"
            );
            assert_eq!(
                msg,
                format!("{prefix}invalid JSON: unexpected end of input"),
                "{name}"
            );
        }
        "json_type" => {
            let ErrorKind::Type {
                path,
                expected,
                got,
            } = jerr.kind()
            else {
                panic!("{name}: not a type error: {msg}");
            };
            assert_eq!(*got, go_kind(string(want, "value")), "{name}: kind");
            assert_eq!(jerr.offset(), offset, "{name}: offset");
            assert!(path.is_root(), "{name}: path");
            assert_eq!(
                msg,
                format!(
                    "{prefix}invalid JSON: the file must be {}, got {}",
                    expected.text(),
                    got.text()
                ),
                "{name}"
            );
            assert_eq!(
                msg,
                format!("{prefix}invalid JSON: the file must be an object, got an array"),
                "{name}"
            );
        }
        "base64" => {
            let ErrorKind::Base64 { path, byte } = jerr.kind() else {
                panic!("{name}: not a base64 error: {msg}");
            };
            assert_eq!(*byte as i64, offset, "{name}: offset");
            assert_eq!(
                msg,
                format!("{prefix}invalid JSON: \"{path}\": invalid base64 at byte {offset}"),
                "{name}"
            );
        }
        other => panic!("{name}: unexpected wrapped kind {other}"),
    }
}

fn check_data(name: &str, d: &Data, want: &Value) {
    let kv: Vec<(String, String)> = array(want, "kv")
        .iter()
        .map(|r| {
            (
                string(r, "key_hex").to_string(),
                string(r, "value_hex").to_string(),
            )
        })
        .collect();
    let got_kv: Vec<(String, String)> =
        d.kv.iter()
            .map(|(k, v)| {
                (
                    common::hex(k),
                    common::hex(v.as_deref().unwrap_or_default()),
                )
            })
            .collect();
    assert_eq!(got_kv, kv, "{name}: kv");

    let exp: Vec<(String, i64)> = array(want, "expirations")
        .iter()
        .map(|r| (string(r, "key_hex").to_string(), dec_i64(r, "at")))
        .collect();
    let got_exp: Vec<(String, i64)> = d
        .expirations
        .iter()
        .map(|(k, at)| (common::hex(k), *at))
        .collect();
    assert_eq!(got_exp, exp, "{name}: expirations");

    let types: Vec<String> = array(want, "types_hex")
        .iter()
        .map(|t| t.as_str().expect("hex").to_string())
        .collect();
    let got_types: Vec<String> = d.types.iter().map(|t| common::hex(t)).collect();
    assert_eq!(got_types, types, "{name}: types");

    let ents = array(want, "entities");
    assert_eq!(ents.len(), d.types.len(), "{name}: entities");
    for (ty, ent) in d.types.iter().zip(ents) {
        assert_eq!(common::hex(ty), string(ent, "entity_hex"), "{name}");
        let docs = d.entities.get(ty).map(Vec::as_slice).unwrap_or_default();
        let want_docs = array(ent, "documents");
        assert_eq!(docs.len(), want_docs.len(), "{name}: documents");
        for (doc, w) in docs.iter().zip(want_docs) {
            assert_eq!(common::hex(&doc.id), string(w, "id_hex"), "{name}");
            let enc = json::marshal(&doc.doc).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(
                String::from_utf8_lossy(&enc),
                string(w, "json"),
                "{name}: document"
            );
        }
    }
    let keys: BTreeSet<&Vec<u8>> = d.entities.keys().collect();
    let listed: BTreeSet<&Vec<u8>> = d.types.iter().collect();
    assert_eq!(keys, listed, "{name}: entity keys");

    assert_eq!(d.replayed as i64, int(want, "replayed"), "{name}: replayed");
}

#[test]
fn golden_legacy_read() {
    let names = case_names(READ);
    assert_eq!(names.len(), 12);

    let mut checked = 0;
    let mut failures = 0;
    for name in &names {
        let rel = format!("{READ}/{name}");
        let want = common::read_json(&format!("{rel}/expected.json"));
        let replay = field(&want, "replay")
            .as_bool()
            .unwrap_or_else(|| panic!("{name}: replay"));
        let now = int(&want, "now");
        assert_eq!(now, 1800000000, "{name}");

        let tmp = TempDir::new();
        let input = common::golden_path(&format!("{rel}/input"));
        let dir = tmp.path().join("input");
        if input.is_dir() {
            copy_tree(&input, &dir);
        } else {
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        }

        assert_eq!(
            Some(legacy::present(&dir)),
            field(&want, "present").as_bool(),
            "{name}: present"
        );

        let at = UNIX_EPOCH + Duration::from_secs(now as u64);
        match (legacy::read(&dir, replay, at), want.get("error")) {
            (Ok(d), None) => check_data(name, &d, field(&want, "data")),
            (Err(err), Some(w)) => {
                let file = common::golden_path(&format!("{rel}/input/{}", string(w, "file")));
                let data = std::fs::read(&file).unwrap_or_else(|e| panic!("{e}"));
                check_error(name, &data, &err, w);
                failures += 1;
            }
            (Ok(_), Some(w)) => panic!("{name}: expected a failure {w:?}"),
            (Err(err), None) => panic!("{name}: {err}"),
        }
        checked += 1;
    }

    assert_eq!(checked, names.len());
    assert_eq!(failures, 3);
}

fn obj(pairs: Vec<(&str, Value)>) -> Map {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn special() -> Data {
    let a_b = "a\u{2028}b".as_bytes().to_vec();
    Data {
        kv: BTreeMap::from([
            (b"plain".to_vec(), Some(b"v".to_vec())),
            (b"bin".to_vec(), Some(vec![0, 0xff, 0x10])),
            (b"<&>".to_vec(), Some(b"html".to_vec())),
            (b"api:entity:skip".to_vec(), Some(b"dropped".to_vec())),
            (b"ttl".to_vec(), Some(b"t".to_vec())),
        ]),
        expirations: BTreeMap::from([(b"ttl".to_vec(), 4102444800), (b"absent".to_vec(), 5)]),
        types: vec![b"book".to_vec(), a_b.clone()],
        entities: BTreeMap::from([
            (
                b"book".to_vec(),
                vec![
                    Document {
                        id: b"1".to_vec(),
                        doc: Some(obj(vec![
                            ("title", Value::String("<b>&".to_string())),
                            ("n", Value::Number(1e21)),
                        ])),
                    },
                    Document {
                        id: "\u{e9}".as_bytes().to_vec(),
                        doc: Some(obj(vec![("x", Value::Null)])),
                    },
                ],
            ),
            (
                a_b,
                vec![Document {
                    id: b"z".to_vec(),
                    doc: Some(Map::new()),
                }],
            ),
        ]),
        replayed: 0,
    }
}

#[test]
fn golden_legacy_write() {
    let names = case_names(WRITE);
    assert_eq!(names, vec!["empty".to_string(), "special".to_string()]);

    let tmp = TempDir::new();
    let mut checked = 0;
    for name in &names {
        let d = match name.as_str() {
            "special" => special(),
            "empty" => Data::default(),
            other => panic!("unexpected case {other}"),
        };
        let out = tmp.path().join(name);
        legacy::write(&out, &d).unwrap_or_else(|e| panic!("{name}: {e}"));

        let golden = common::golden_path(&format!("{WRITE}/{name}"));
        let files = super::support::read_tree(&golden);
        assert_eq!(files.len(), 3, "{name}");
        assert_same_tree(&out, &golden, name);
        checked += 1;
    }

    assert_eq!(checked, names.len());
}

fn write_input(dir: &std::path::Path, name: &str, data: &[u8]) {
    std::fs::write(dir.join(name), data).unwrap_or_else(|e| panic!("{name}: {e}"));
}

fn nested(depth: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(depth * 6 + 1);
    for _ in 0..depth {
        out.extend_from_slice(br#"{"a":"#);
    }
    out.push(b'1');
    out.extend(std::iter::repeat_n(b'}', depth));
    out
}

#[test]
fn legacy_documents_nested_on_10000_levels_round_trip() {
    let depth = 9998;
    let doc = nested(depth);

    let tmp = TempDir::new();
    let input = tmp.path().join("input");
    std::fs::create_dir_all(&input).unwrap_or_else(|e| panic!("{e}"));

    let mut docs = br#"{"api:entity:book:id:1":"#.to_vec();
    docs.extend_from_slice(&doc);
    docs.push(b'}');
    write_input(&input, legacy::JSON_DATA_FILE, &docs);

    let mut log = br#"{"op":"put","key":"api:entity:book:id:2","value":"#.to_vec();
    log.extend_from_slice(&doc);
    log.extend_from_slice(b"}\n");
    write_input(&input, legacy::JSON_RECOVERY_FILE, &log);

    let d = legacy::read(&input, true, UNIX_EPOCH).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.replayed, 1);
    let books = &d.entities[b"book".as_slice()];
    assert_eq!(books.len(), 2);
    for b in books {
        let enc = json::marshal(&b.doc).unwrap_or_else(|e| panic!("{e}"));
        assert!(enc == doc, "document {:?} changed", b.id);
    }

    let out = tmp.path().join("out");
    legacy::write(&out, &d).unwrap_or_else(|e| panic!("{e}"));
    let back = legacy::read(&out, false, UNIX_EPOCH).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        back.entities == d.entities,
        "the export reads back differently"
    );
    assert_eq!(back.types, d.types);

    let mut deeper = br#"{"api:entity:book:id:1":"#.to_vec();
    deeper.extend_from_slice(&nested(10_000));
    deeper.push(b'}');
    write_input(&input, legacy::JSON_DATA_FILE, &deeper);
    let err = legacy::read(&input, false, UNIX_EPOCH).expect_err("too deep");
    let at = r#"{"api:entity:book:id:1":"#.len() + 9_999 * r#"{"a":"#.len() + 1;
    assert_eq!(
        err.to_string(),
        format!("elysiandbjson.json: invalid JSON at byte {at}: nesting too deep")
    );
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(s) => *s,
        Err(payload) => match payload.downcast::<&str>() {
            Ok(s) => s.to_string(),
            Err(_) => "<not a string>".to_string(),
        },
    }
}

#[test]
fn legacy_null_files_panic_on_a_replayed_put_like_go() {
    let cases: [(&str, &str, &[u8], &str); 2] = [
        (
            legacy::DATA_FILE,
            legacy::STORE_RECOVERY_FILE,
            br#"{"op":"put","key":"k","value":"dg=="}"#,
            "elysiandb.json holds null, a logged key cannot be stored in it",
        ),
        (
            legacy::JSON_DATA_FILE,
            legacy::JSON_RECOVERY_FILE,
            br#"{"op":"put","key":"api:entity:book:id:1","value":{}}"#,
            "elysiandbjson.json holds null, a logged document cannot be stored in it",
        ),
    ];

    let mut checked = 0;
    for (file, log, line, want) in cases {
        let tmp = TempDir::new();
        write_input(tmp.path(), file, b" null\n");
        let mut deletes = br#"{"op":"del","key":"k"}"#.to_vec();
        deletes.push(b'\n');
        write_input(tmp.path(), log, &deletes);

        let d = legacy::read(tmp.path(), true, UNIX_EPOCH).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.replayed, 1, "{file}");
        let d = legacy::read(tmp.path(), false, UNIX_EPOCH).unwrap_or_else(|e| panic!("{e}"));
        assert!(d.kv.is_empty() && d.types.is_empty(), "{file}");

        deletes.extend_from_slice(line);
        write_input(tmp.path(), log, &deletes);
        let dir = tmp.path().to_path_buf();
        match std::panic::catch_unwind(move || legacy::read(&dir, true, UNIX_EPOCH)) {
            Ok(res) => panic!("{file}: expected a panic, got {res:?}"),
            Err(payload) => assert_eq!(panic_text(payload), want),
        }
        checked += 1;
    }
    assert_eq!(checked, cases.len());
}

#[test]
fn legacy_expirations_follow_the_d20_order() {
    let tmp = TempDir::new();
    write_input(
        tmp.path(),
        legacy::DATA_FILE,
        br#"{"j":"ag==","k":"aw==","x":"eA=="}"#,
    );
    write_input(
        tmp.path(),
        legacy::EXPIRATION_FILE,
        br#"{"5":["k","j"],"20":["j"],"12":["k"],"+12":["x"]}"#,
    );

    let d = legacy::read(tmp.path(), false, UNIX_EPOCH).unwrap_or_else(|e| panic!("{e}"));
    let want = BTreeMap::from([(b"j".to_vec(), 20), (b"k".to_vec(), 12)]);
    assert_eq!(d.expirations, want);
    assert_eq!(d.kv.len(), 3);

    let raw = legacy::read_expirations_file(&tmp.path().join(legacy::EXPIRATION_FILE))
        .unwrap_or_else(|e| panic!("{e}"));
    let want_raw = BTreeMap::from([
        (5, vec![b"k".to_vec(), b"j".to_vec()]),
        (12, vec![b"k".to_vec()]),
        (20, vec![b"j".to_vec()]),
    ]);
    assert_eq!(raw, want_raw);
}

#[test]
fn legacy_messages_follow_d27() {
    let cases: [(&str, &[u8], &str); 5] = [
        (
            legacy::DATA_FILE,
            br#"{"session:1":"abcd%"}"#,
            r#"elysiandb.json: invalid JSON: "session:1": invalid base64 at byte 4"#,
        ),
        (
            legacy::JSON_DATA_FILE,
            br#"{"api:entity:book:id:b1":"x"}"#,
            r#"elysiandbjson.json: invalid JSON: "api:entity:book:id:b1" must be an object, got a string"#,
        ),
        (
            legacy::DATA_FILE,
            br#"["a"]"#,
            "elysiandb.json: invalid JSON: the file must be an object, got an array",
        ),
        (
            legacy::EXPIRATION_FILE,
            br#"{"12":"k"}"#,
            r#"elysiandb.expiration.json: invalid JSON: "12" must be an array of strings, got a string"#,
        ),
        (
            legacy::DATA_FILE,
            br#"{"k":x}"#,
            "elysiandb.json: invalid JSON at byte 6: unexpected character 'x'",
        ),
    ];

    let mut checked = 0;
    for (file, data, want) in cases {
        let tmp = TempDir::new();
        write_input(tmp.path(), file, data);
        match legacy::read(tmp.path(), true, UNIX_EPOCH) {
            Ok(d) => panic!("{want}: read {d:?}"),
            Err(err) => assert_eq!(err.to_string(), want),
        }
        checked += 1;
    }
    assert_eq!(checked, cases.len());

    let tmp = TempDir::new();
    let dir = tmp.path().join(legacy::DATA_FILE);
    std::fs::create_dir(&dir).unwrap_or_else(|e| panic!("{e}"));
    let err = legacy::read(tmp.path(), true, UNIX_EPOCH).expect_err("a folder is not a file");
    assert_eq!(
        err.to_string(),
        format!(
            "elysiandb.json: read {}: Is a directory (os error 21)",
            dir.display()
        )
    );
}
