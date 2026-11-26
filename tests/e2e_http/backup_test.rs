use std::collections::BTreeMap;
use std::io::{Read, Write};

use elysiandb::archive::gzip::{self, BEST_SPEED};
use elysiandb::archive::tar::{self, Header, TYPE_REG};
use elysiandb::crypto::sha256;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::storage;

use crate::client::Multipart;
use crate::harness::{ApiServer, start_admin_session, vs};

#[derive(Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub data: Vec<u8>,
}

pub fn read_archive(archive: &[u8]) -> Vec<ArchiveEntry> {
    let gz = gzip::new_reader(archive)
        .unwrap_or_else(|err| panic!("the backup is not a gzip file: {err}"));

    let mut entries = Vec::new();
    let mut tr = tar::Reader::new(gz);
    loop {
        let header = match tr.next() {
            Ok(Some(header)) => header,
            Ok(None) => return entries,
            Err(err) => panic!("the backup is not a tar archive: {err}"),
        };

        let mut data = Vec::new();
        let _ = tr.read_to_end(&mut data);
        entries.push(ArchiveEntry {
            name: String::from_utf8_lossy(&header.name).into_owned(),
            data,
        });
    }
}

pub fn write_archive(entries: &[ArchiveEntry]) -> Vec<u8> {
    let gz = gzip::Writer::new_level(Vec::new(), BEST_SPEED)
        .unwrap_or_else(|err| panic!("cannot start the gzip stream: {err}"));
    let mut tw = tar::Writer::new(gz);
    for e in entries {
        if let Err(err) = tw.write_header(&Header {
            name: e.name.as_bytes().to_vec(),
            mode: 0o600,
            size: e.data.len() as i64,
            typeflag: TYPE_REG,
            ..Header::default()
        }) {
            panic!("cannot write {}: {err}", e.name);
        }

        let _ = tw.write_all(&e.data);
    }

    let _ = tw.close();
    let mut gz = tw.into_inner();
    let _ = gz.close();

    gz.get_ref().clone()
}

fn strip_created_at(text: &str) -> String {
    const KEY: &str = "\"created_at\": \"";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(KEY) {
        let after = &rest[i + KEY.len()..];
        match after.find('"') {
            Some(end) => {
                out.push_str(&rest[..i]);
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

pub fn archive_content(archive: &[u8]) -> BTreeMap<String, String> {
    let mut content = BTreeMap::new();
    for e in read_archive(archive) {
        content.insert(e.name, strip_created_at(&String::from_utf8_lossy(&e.data)));
    }

    content
}

fn mutate_archive(archive: &[u8], mutate: impl Fn(&str, Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let mut entries = read_archive(archive);
    for e in entries.iter_mut() {
        e.data = mutate(&e.name, std::mem::take(&mut e.data));
    }

    write_archive(&entries)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn rewrite_archive(archive: &[u8], mutate: impl Fn(&str, Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let mut entries = read_archive(archive);
    let mut sums = Map::new();
    for e in entries.iter_mut() {
        e.data = mutate(&e.name, std::mem::take(&mut e.data));
        if e.name != "manifest.json" {
            sums.insert(e.name.clone(), vs(&hex(&sha256::sum256(&e.data))));
        }
    }

    let mut manifest = Map::new();
    if let Err(err) = json::unmarshal(&entries[0].data, &mut manifest) {
        panic!("invalid manifest: {err}");
    }

    manifest.insert("files".to_string(), Value::Object(sums));
    entries[0].data = json::marshal(&manifest).unwrap_or_default();

    write_archive(&entries)
}

fn replace_first(data: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    if old.is_empty() {
        let mut out = new.to_vec();
        out.extend_from_slice(data);
        return out;
    }
    match data.windows(old.len()).position(|w| w == old) {
        Some(i) => {
            let mut out = data[..i].to_vec();
            out.extend_from_slice(new);
            out.extend_from_slice(&data[i + old.len()..]);
            out
        }
        None => data.to_vec(),
    }
}

fn replace_entry(entry: &str, content: &str) -> impl Fn(&str, Vec<u8>) -> Vec<u8> {
    let entry = entry.to_string();
    let content = content.to_string();
    move |name: &str, data: Vec<u8>| {
        if name != entry {
            return data;
        }

        content.as_bytes().to_vec()
    }
}

fn replace_in(entry: &str, old: &str, new: &str) -> impl Fn(&str, Vec<u8>) -> Vec<u8> {
    let entry = entry.to_string();
    let old = old.to_string();
    let new = new.to_string();
    move |name: &str, data: Vec<u8>| {
        if name != entry {
            return data;
        }

        replace_first(&data, old.as_bytes(), new.as_bytes())
    }
}

pub fn populate_backup_source() -> (ApiServer, BTreeMap<&'static str, String>) {
    let s = start_admin_session(None);

    s.must_do(
        200,
        "PUT",
        "/api/security/user/admin/password",
        r#"{"password":"admin-secret"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"alice","password":"alice-pw","role":"admin"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"carol","password":"carol-pw","role":"user"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/book/create",
        r#"{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"}}}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/empty/create",
        r#"{"fields":{"x":{"type":"string","required":true}}}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"id":"b2","title":"Dune","year":1965}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/book",
        r#"{"id":"b1","title":"Alien"}"#,
        &[],
    );
    s.must_do(
        200,
        "POST",
        "/api/movie",
        r#"{"id":"m1","title":"Heat","tags":["a","b"],"meta":{"rating":4.5}}"#,
        &[],
    );
    s.must_do(200, "POST", "/api/hook/book", r#"{"name":"tag","event":"post_read","language":"javascript","priority":5,"enabled":true,"script":"function postRead(ctx) { ctx.entity.tagged = true }"}"#, &[]);
    s.must_do(200, "PUT", "/api/acl/carol/book", r#"{"permissions":{"read":true,"create":false,"update":false,"delete":false,"owning_read":true,"owning_write":true,"owning_update":true,"owning_delete":true}}"#, &[]);
    s.must_do(204, "PUT", "/kv/app:marker", "v1", &[]);
    s.must_do(204, "PUT", "/kv/app:session?ttl=3600", "short-lived", &[]);
    s.must_do(204, "PUT", "/kv/app:binary", b"\xff\xfe\x00raw", &[]);

    let mut state = BTreeMap::new();
    for (name, path) in [
        ("book", "/api/book"),
        ("book schema", "/api/book/schema"),
        ("types", "/api/entity/types"),
        ("hooks", "/api/hook/book"),
        ("acl alice", "/api/acl/alice"),
        ("acl carol", "/api/acl/carol"),
        ("users", "/api/security/user"),
    ] {
        state.insert(name, s.must_do(200, "GET", path, "", &[]).text());
    }

    (s, state)
}

fn is_backup_disposition(got: &str) -> bool {
    let Some(rest) = got.strip_prefix("attachment; filename=\"elysiandb-backup-") else {
        return false;
    };
    let Some(stamp) = rest.strip_suffix(".tar.gz\"") else {
        return false;
    };
    let b = stamp.as_bytes();
    b.len() == 16
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'T'
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'Z'
}

#[test]
fn backup_and_restore_round_trip() {
    let (_source, state) = populate_backup_source();
    let session_expiration = storage::get_key_expiration(b"app:session").unwrap_or(0);

    let resp = _source.must_do(200, "GET", "/api/backup", "", &[]);
    let got = resp.header("Content-Type");
    if got != "application/gzip" {
        panic!("expected application/gzip, got {got:?}");
    }

    let got = resp.header("Content-Disposition");
    if !is_backup_disposition(&got) {
        panic!("unexpected Content-Disposition {got:?}");
    }

    let archive = resp.body;

    let target = start_admin_session(None);
    target.must_do(200, "POST", "/api/stale", r#"{"id":"s1"}"#, &[]);
    target.must_do(204, "PUT", "/kv/target:only", "x", &[]);

    let report = target
        .must_do(
            200,
            "POST",
            "/api/restore",
            &archive,
            &["Content-Type", "application/gzip"],
        )
        .object();
    let entities = match report.get("entities") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    let num = |v: Option<&Value>| v.and_then(Value::as_f64);
    if num(entities.get("book")) != Some(2.0)
        || num(entities.get("empty")) != Some(0.0)
        || num(entities.get("movie")) != Some(1.0)
        || num(report.get("kv_keys")) != Some(3.0)
    {
        panic!("unexpected restore report {report:?}");
    }

    target.must_do(401, "GET", "/api/book", "", &[]);

    let anonymous = target.anonymous();
    for (username, password, status) in [
        ("alice", "alice-pw", 200),
        ("admin", "admin-secret", 200),
        ("admin", "admin", 401),
    ] {
        let mut body = Map::new();
        body.insert("password".to_string(), vs(password));
        body.insert("username".to_string(), vs(username));
        let body = json::marshal(&body).unwrap_or_default();
        anonymous.must_do(status, "POST", "/api/security/login", body, &[]);
    }

    let mut restored = target.anonymous();
    restored.login("admin", "admin-secret");

    for (name, path) in [
        ("acl alice", "/api/acl/alice"),
        ("acl carol", "/api/acl/carol"),
        ("book", "/api/book"),
        ("book schema", "/api/book/schema"),
        ("hooks", "/api/hook/book"),
        ("types", "/api/entity/types"),
        ("users", "/api/security/user"),
    ] {
        let got = restored.must_do(200, "GET", path, "", &[]).text();
        if got != state[name] {
            panic!(
                "{name} changed after the restore:\nbefore {}\nafter  {got}",
                state[name]
            );
        }
    }

    if !state["book"].contains(r#""tagged":true"#) {
        panic!(
            "expected the hook to run on the source, got {}",
            state["book"]
        );
    }

    let names = restored
        .must_do(200, "GET", "/api/entity/types/name", "", &[])
        .object();
    if names.get("entities") != Some(&Value::Array(vec![vs("book"), vs("empty"), vs("movie")])) {
        panic!(
            "expected the entity types of the backup only, got {:?}",
            names.get("entities")
        );
    }

    restored.must_do(200, "GET", "/api/empty/schema", "", &[]);
    restored.must_do(400, "POST", "/api/book", r#"{"id":"b3"}"#, &[]);
    restored.must_do(400, "POST", "/api/empty", r#"{"id":"e1"}"#, &[]);
    restored.must_do(404, "GET", "/api/stale/s1", "", &[]);
    restored.must_do(404, "GET", "/kv/target:only", "", &[]);

    let kv: [(&str, &[u8]); 3] = [
        ("app:binary", b"\xff\xfe\x00raw"),
        ("app:marker", b"v1"),
        ("app:session", b"short-lived"),
    ];
    for (key, value) in kv {
        let got = storage::get_by_key(key.as_bytes());
        match &got {
            Ok(v) if v.as_slice() == value => {}
            _ => panic!("expected {key}={value:?} after the restore, got {got:?}"),
        }
    }

    match storage::get_key_expiration(b"app:session") {
        Some(got) if got == session_expiration => {}
        got => {
            panic!("expected app:session to keep its expiration {session_expiration}, got {got:?}")
        }
    }

    if storage::get_key_expiration(b"app:marker").is_some() {
        panic!("app:marker must not get an expiration");
    }

    let again = restored.must_do(200, "GET", "/api/backup", "", &[]).body;
    if archive_content(&again) != archive_content(&archive) {
        panic!("a second backup must hold the same content as the first");
    }

    restored.must_do(
        200,
        "POST",
        "/api/security/user",
        r#"{"username":"bob","password":"bob-pass","role":"admin"}"#,
        &[],
    );
    let acl = restored
        .must_do(200, "GET", "/api/acl/bob/book", "", &[])
        .object();
    if acl.get("username") != Some(&vs("bob")) {
        panic!("expected a new user to get ACLs after the restore, got {acl:?}");
    }

    restored.must_do(
        200,
        "POST",
        "/api/film",
        r#"{"id":"f1","title":"Alien"}"#,
        &[],
    );
    restored.must_do(200, "GET", "/api/film/f1", "", &[]);
    let acl = restored
        .must_do(200, "GET", "/api/acl/alice/film", "", &[])
        .object();
    if acl.get("entity") != Some(&vs("film")) {
        panic!("expected a new entity type to get ACLs after the restore, got {acl:?}");
    }
}

#[test]
fn restore_accepts_any_file_name() {
    let (source, _) = populate_backup_source();
    let archive = source.must_do(200, "GET", "/api/backup", "", &[]).body;

    let target = start_admin_session(None);

    let mut form = Multipart::new();
    form.file("anything", "my-copy.bin", &archive);
    let content_type = form.content_type();
    let body = form.close();

    target.must_do(
        200,
        "POST",
        "/api/restore",
        body,
        &["Content-Type", &content_type],
    );

    let mut restored = target.anonymous();
    restored.login("alice", "alice-pw");
    restored.must_do(200, "GET", "/api/book/b1", "", &[]);
}

fn gzip_bytes(data: &[u8]) -> Vec<u8> {
    let mut gz = gzip::Writer::new_level(Vec::new(), BEST_SPEED)
        .unwrap_or_else(|err| panic!("cannot start the gzip stream: {err}"));
    let _ = gz.write_all(data);
    let _ = gz.close();

    gz.get_ref().clone()
}

fn read_entry(entries: &[ArchiveEntry], name: &str) -> Vec<u8> {
    for e in entries {
        if e.name == name {
            return e.data.clone();
        }
    }

    panic!("{name} is not in the archive");
}

fn drop_checksums(name: &str, data: Vec<u8>) -> Vec<u8> {
    if name != "manifest.json" {
        return data;
    }

    let mut manifest = Map::new();
    let _ = json::unmarshal(&data, &mut manifest);
    manifest.insert("files".to_string(), Value::Object(Map::new()));

    json::marshal(&manifest).unwrap_or_default()
}

fn split_after_newline(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('\n') {
        out.push(rest[..=i].to_string());
        rest = &rest[i + 1..];
    }
    out.push(rest.to_string());
    out
}

#[test]
fn restore_refuses_invalid_archives_and_keeps_the_data() {
    let (source, _) = populate_backup_source();
    let archive = source.must_do(200, "GET", "/api/backup", "", &[]).body;

    let target = start_admin_session(None);
    target.must_do(200, "POST", "/api/keep", r#"{"id":"k1","v":1}"#, &[]);
    target.must_do(204, "PUT", "/kv/keep:me", "x", &[]);

    let entries = read_archive(&archive);
    let book_lines = split_after_newline(&String::from_utf8_lossy(&read_entry(
        &entries,
        "entities/book.jsonl",
    )));

    let mut extra = entries.clone();
    extra.push(ArchiveEntry {
        name: "extra.txt".to_string(),
        data: b"x".to_vec(),
    });
    let mut wrong_order = vec![entries[0].clone(), entries[2].clone(), entries[1].clone()];
    wrong_order.extend_from_slice(&entries[3..]);
    let doubled = format!("{}{}", book_lines[0], book_lines[0]);

    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "bad checksum",
            mutate_archive(&archive, replace_in("entities/book.jsonl", "Dune", "Dunx")),
            "checksum mismatch for entities/book.jsonl",
        ),
        (
            "count mismatch",
            rewrite_archive(
                &archive,
                replace_in("entities/book.jsonl", &book_lines[0], ""),
            ),
            "entities/book.jsonl holds 1 entries, the manifest announces 2",
        ),
        (
            "document without id",
            rewrite_archive(
                &archive,
                replace_in("entities/book.jsonl", r#""id":"b2""#, r#""id":"""#),
            ),
            "a document must be an object with a string id",
        ),
        (
            "duplicate id",
            rewrite_archive(
                &archive,
                replace_in("entities/book.jsonl", &book_lines[0], &doubled),
            ),
            r#"duplicate id "b2""#,
        ),
        ("empty body", Vec::new(), "must be a backup archive"),
        (
            "empty key",
            rewrite_archive(&archive, replace_entry("security/users.key", "\n")),
            "does not hold a valid key",
        ),
        (
            "extra file",
            write_archive(&extra),
            "unexpected file extra.txt",
        ),
        (
            "foreign schema",
            rewrite_archive(
                &archive,
                replace_in("schemas/book.json", r#""id":"book""#, r#""id":"film""#),
            ),
            r#"describes "film" instead of "book""#,
        ),
        (
            "gzip without tar",
            gzip_bytes(b"plain text"),
            "invalid backup archive",
        ),
        (
            "internal KV key",
            rewrite_archive(
                &archive,
                replace_in(
                    "kv/keys.jsonl",
                    r#""app:marker""#,
                    r#""api:entity:types:list""#,
                ),
            ),
            "invalid key",
        ),
        (
            "invalid document",
            rewrite_archive(&archive, replace_in("entities/book.jsonl", "{", "[")),
            "entities/book.jsonl, document 1",
        ),
        (
            "missing file",
            write_archive(&entries[..entries.len() - 1]),
            "kv/keys.jsonl is missing",
        ),
        (
            "newer format",
            mutate_archive(
                &archive,
                replace_in(
                    "manifest.json",
                    r#""format_version": 1"#,
                    r#""format_version": 2"#,
                ),
            ),
            "format version 2 is newer",
        ),
        (
            "no checksums",
            mutate_archive(&archive, drop_checksums),
            "no checksum for security/users.key",
        ),
        ("not a gzip", b"not an archive".to_vec(), "not a gzip file"),
        (
            "truncated",
            archive[..archive.len() / 2].to_vec(),
            "invalid backup archive",
        ),
        (
            "unknown KV encoding",
            rewrite_archive(
                &archive,
                replace_in(
                    "kv/keys.jsonl",
                    r#""encoding":"base64""#,
                    r#""encoding":"hex""#,
                ),
            ),
            r#"unknown encoding "hex""#,
        ),
        (
            "unknown KV field",
            rewrite_archive(
                &archive,
                replace_in("kv/keys.jsonl", r#"{"key""#, r#"{"extra":1,"key""#),
            ),
            "unknown field",
        ),
        (
            "unknown format",
            mutate_archive(
                &archive,
                replace_in("manifest.json", r#""elysiandb-backup""#, r#""other""#),
            ),
            r#"unknown format "other""#,
        ),
        (
            "wrong order",
            write_archive(&wrong_order),
            "expected security/users.key",
        ),
    ];

    for (name, body, message) in cases {
        let resp = target.must_do(400, "POST", "/api/restore", &body, &[]);
        let got = match resp.object().get("error") {
            Some(Value::String(m)) => m.clone(),
            _ => String::new(),
        };
        if !got.contains(message) {
            panic!(
                "{name}: expected an error containing {message:?}, got {}",
                resp.text()
            );
        }

        let v = target
            .must_do(200, "GET", "/api/keep/k1", "", &[])
            .object()
            .get("v")
            .and_then(Value::as_f64);
        if v != Some(1.0) {
            panic!("{name}: a refused restore must leave the data untouched, got {v:?}");
        }

        target.must_do(200, "GET", "/kv/keep:me", "", &[]);
        target.must_do(404, "GET", "/api/book/b1", "", &[]);
    }
}

#[test]
fn backup_and_restore_require_an_admin() {
    let s = start_admin_session(None);
    let archive = s.must_do(200, "GET", "/api/backup", "", &[]).body;

    let anonymous = s.anonymous();
    anonymous.must_do(401, "GET", "/api/backup", "", &[]);
    anonymous.must_do(401, "POST", "/api/restore", &archive, &[]);
}
