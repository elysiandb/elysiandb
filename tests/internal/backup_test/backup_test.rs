use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

use elysiandb::acl::{self, Permission};
use elysiandb::api;
use elysiandb::archive::gzip::{self, BEST_SPEED};
use elysiandb::archive::tar::{self, Header, TYPE_REG};
use elysiandb::backup::{self, ArchiveError, KvEntry, Manifest, Report};
use elysiandb::configuration::Config;
use elysiandb::crypto::sha256;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::time::{Location, Month, Time};
use elysiandb::schema;
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::storage;

use crate::tempdir::TempDir;

struct Entry {
    name: String,
    data: Vec<u8>,
}

#[derive(Default)]
struct Folders {
    dirs: Vec<TempDir>,
}

impl Folders {
    fn temp(&mut self) -> PathBuf {
        let dir = TempDir::new();
        let path = dir.path().to_path_buf();
        self.dirs.push(dir);
        path
    }

    fn setup(&mut self, mode: &str) -> PathBuf {
        let folder = self.temp();
        let mut cfg = Config::default();
        cfg.store.folder = folder.to_string_lossy().into_owned();
        cfg.store.shards = 4;
        cfg.store.crash_recovery.enabled = true;
        cfg.store.crash_recovery.max_log_mb = 10;
        cfg.api.schema.enabled = true;
        cfg.api.schema.strict = true;
        cfg.security.authentication.enabled = !mode.is_empty();
        cfg.security.authentication.mode = mode.to_string();
        globals::set_config(Some(Arc::new(cfg)));

        storage::load_db();
        storage::load_json_db();

        if let Err(err) = security::init_admin_user_if_not_exists() {
            panic!("cannot create the default admin: {err}");
        }

        let _ = acl::init_acl();

        folder
    }
}

impl Drop for Folders {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn object(text: &str) -> Map {
    let mut m = Map::new();
    json::unmarshal(text.as_bytes(), &mut m).unwrap_or_else(|e| panic!("{text}: {e}"));
    m
}

fn backup_time() -> Time {
    Time::from_date(2026, Month(11), 1, 9, 14, 0, 0, &Location::utc())
}

fn create() -> Vec<u8> {
    let mut buf = Vec::new();
    if let Err(err) = backup::create(&mut buf, backup_time()) {
        panic!("backup failed: {err}");
    }

    buf
}

fn restore(archive: &[u8]) -> Report {
    match backup::restore(std::io::Cursor::new(archive.to_vec())) {
        Ok(report) => report,
        Err(err) => panic!("restore failed: {err}"),
    }
}

fn entries(archive: &[u8]) -> Vec<Entry> {
    let gz = match gzip::new_reader(archive) {
        Ok(gz) => gz,
        Err(err) => panic!("not a gzip archive: {err}"),
    };

    let mut out = Vec::new();
    let mut tr = tar::Reader::new(gz);
    loop {
        let header = match tr.next() {
            Ok(Some(header)) => header,
            Ok(None) => return out,
            Err(err) => panic!("not a tar archive: {err}"),
        };

        let mut data = Vec::new();
        let _ = tr.read_to_end(&mut data);
        out.push(Entry {
            name: String::from_utf8_lossy(&header.name).into_owned(),
            data,
        });
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn rebuild(archive: &[u8], mutate: impl Fn(&str, Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let mut list = entries(archive);
    let mut manifest = Map::new();
    let _ = json::unmarshal(&list[0].data, &mut manifest);

    let mut sums = Map::new();
    for e in list[1..].iter_mut() {
        e.data = mutate(&e.name, std::mem::take(&mut e.data));
        sums.insert(e.name.clone(), Value::String(hex(&sha256::sum256(&e.data))));
    }

    manifest.insert("files".to_string(), Value::Object(sums));
    list[0].data = json::marshal(&manifest).expect("manifest");

    let gz = gzip::Writer::new_level(Vec::new(), BEST_SPEED).expect("gzip");
    let mut tw = tar::Writer::new(gz);
    for e in &list {
        let _ = tw.write_header(&Header {
            name: e.name.as_bytes().to_vec(),
            mode: 0o600,
            size: e.data.len() as i64,
            typeflag: TYPE_REG,
            ..Header::default()
        });
        let _ = tw.write_all(&e.data);
    }

    let _ = tw.close();
    let mut gz = tw.into_inner();
    let _ = gz.close();

    gz.get_ref().clone()
}

fn manifest_of(archive: &[u8]) -> Manifest {
    let mut manifest = Manifest::default();
    if let Err(err) = json::unmarshal(&entries(archive)[0].data, &mut manifest) {
        panic!("invalid manifest: {err}");
    }

    manifest
}

fn names(list: &[Entry]) -> Vec<String> {
    list.iter().map(|e| e.name.clone()).collect()
}

#[test]
fn create_writes_the_manifest_first_and_checksums_every_file() {
    let mut folders = Folders::default();
    folders.setup("user");

    api::update_entity_schema(
        b"book",
        &object(r#"{"title":{"type":"string","required":true}}"#),
    );
    api::write_entity(b"book", &mut object(r#"{"id":"b1","title":"Dune"}"#));
    api::add_entity_type(b"empty");
    storage::put_key_value(b"app:key", b"v").expect("put");

    let archive = create();
    let list = entries(&archive);
    if list[0].name != backup::MANIFEST_PATH
        || list[1].name != backup::KEY_PATH
        || list[list.len() - 1].name != backup::KV_PATH
    {
        panic!("unexpected archive layout {:?}", names(&list));
    }

    let manifest = manifest_of(&archive);
    if manifest.format != backup::FORMAT
        || manifest.format_version != backup::FORMAT_VERSION
        || manifest.engine != "internal"
        || manifest.created_at != "2026-11-01T09:14:00Z"
        || manifest.kv_keys != 1
    {
        panic!("unexpected manifest {manifest:?}");
    }

    if manifest.files.len() != list.len() - 1 {
        panic!(
            "expected a checksum for every file, got {:?} for {:?}",
            manifest.files,
            names(&list)
        );
    }

    for e in &list[1..] {
        if manifest.files.get(&e.name) != Some(&hex(&sha256::sum256(&e.data))) {
            panic!("wrong checksum for {}", e.name);
        }
    }

    let mut counts: BTreeMap<Vec<u8>, i64> = BTreeMap::new();
    for e in &manifest.entities {
        counts.insert(e.name.as_bytes().to_vec(), e.documents);
    }

    if counts.get(b"book".as_slice()) != Some(&1)
        || counts.get(b"empty".as_slice()) != Some(&0)
        || counts.get(security::USER_ENTITY.as_bytes()) != Some(&1)
    {
        panic!("unexpected document counts {counts:?}");
    }

    if counts.contains_key(schema::SCHEMA_ENTITY.as_bytes()) {
        panic!("schemas must not be listed as an entity: {counts:?}");
    }

    let key = security::create_key_file_or_get_key().expect("key");
    if list[1].data != key {
        panic!("expected the users key in the archive");
    }

    if !names(&list).join(",").contains("schemas/book.json") {
        panic!(
            "expected the book schema in the archive, got {:?}",
            names(&list)
        );
    }
}

#[test]
fn file_name_is_utc_in_basic_format() {
    let paris = Location::fixed("CET", 3600);
    let got = backup::file_name(&Time::from_date(2026, Month(11), 1, 10, 14, 0, 0, &paris));
    if got != "elysiandb-backup-20261101T091400Z.tar.gz" {
        panic!("unexpected file name {got}");
    }
}

#[test]
fn entity_names_are_escaped_in_paths() {
    let mut folders = Folders::default();
    folders.setup("");

    let entities = ["a/b c", "../x", "Ω"];
    for entity in entities {
        let mut doc = Map::new();
        doc.insert("id".to_string(), Value::String("1".to_string()));
        doc.insert("v".to_string(), Value::String(entity.to_string()));
        api::write_entity(entity.as_bytes(), &mut doc);
    }

    let archive = create();
    for entity in entities {
        for path in [
            backup::entity_path(entity.as_bytes()),
            backup::schema_path(entity.as_bytes()),
        ] {
            let (folder, file) = path.split_once('/').unwrap_or((path.as_str(), ""));
            let listed = names(&entries(&archive)).join(",");
            if (folder != "entities" && folder != "schemas")
                || file.contains(['/', ' '])
                || !listed.contains(&path)
            {
                panic!("unsafe or missing path {path:?} in {listed}");
            }
        }
    }

    folders.setup("");
    restore(&archive);

    for entity in entities {
        let got = api::read_entity_by_id(entity.as_bytes(), b"1");
        let restored = got
            .as_ref()
            .and_then(|m| m.get("v"))
            .and_then(Value::as_str)
            == Some(entity);
        if !restored {
            panic!("expected {entity:?} to be restored, got {got:?}");
        }
    }
}

fn list_ids(entity: &str) -> Vec<String> {
    api::list_entities(entity.as_bytes(), 0, 0, b"", true, None, b"", b"")
        .iter()
        .map(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[test]
fn restore_replaces_everything_and_persists() {
    let mut folders = Folders::default();
    folders.setup("user");

    if let Err(err) = security::create_basic_user(&BasicUser {
        username: b"alice".to_vec(),
        password: b"alice-pw".to_vec(),
        role: Role::admin(),
    }) {
        panic!("cannot create alice: {err}");
    }

    api::write_entity(b"book", &mut object(r#"{"id":"b2","title":"Dune"}"#));
    api::write_entity(b"book", &mut object(r#"{"id":"b1","title":"Alien"}"#));
    let _ = acl::init_acl();
    storage::put_key_value(b"app:plain", b"v1").expect("put");
    storage::put_key_value(b"app:binary", &[0xff, 0x00, 0xfe]).expect("put");
    let expires_at = Time::now().unix() + 3600;
    storage::put_key_value_with_expiration(b"app:ttl", b"soon", expires_at).expect("put");
    let archive = create();

    let folder = folders.setup("user");
    api::write_entity(b"stale", &mut object(r#"{"id":"s1"}"#));
    storage::put_key_value(b"target:only", b"x").expect("put");
    if let Err(err) = security::create_session(
        b"admin",
        Role::admin(),
        std::time::Duration::from_secs(3600),
    ) {
        panic!("cannot create a session: {err}");
    }

    let report = restore(&archive);
    if report.entities.get(b"book".as_slice()) != Some(&2)
        || report.kv_keys != 3
        || report.expired_kv_keys != 0
    {
        panic!("unexpected report {report:?}");
    }

    let sessions = folder.join(security::SESSIONS_FILENAME);
    match std::fs::metadata(&sessions) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        other => panic!("expected the restore to end every session, got {other:?}"),
    }

    storage::load_db();
    storage::load_json_db();

    if security::authenticate_user(b"alice", b"alice-pw").is_none() {
        panic!("expected alice to log in with her password after the restore");
    }

    let ids = list_ids("book");
    if ids != ["b2", "b1"] {
        panic!("expected the list order of the backup, got {ids:?}");
    }

    if api::read_entity_by_id(b"stale", b"s1").is_some() || api::entity_type_exists(b"stale") {
        panic!("data that is not in the archive must be gone");
    }

    if storage::get_by_key(b"target:only").is_ok() {
        panic!("KV keys that are not in the archive must be gone");
    }

    let expected: [(&[u8], &[u8]); 3] = [
        (b"app:plain", b"v1"),
        (b"app:binary", &[0xff, 0x00, 0xfe]),
        (b"app:ttl", b"soon"),
    ];
    for (key, value) in expected {
        match storage::get_by_key(key) {
            Ok(got) if got == value => {}
            other => panic!(
                "expected {}={value:?} after the restore, got {other:?}",
                String::from_utf8_lossy(key)
            ),
        }
    }

    let got = storage::get_key_expiration(b"app:ttl");
    if got != Some(expires_at) {
        panic!("expected app:ttl to expire at {expires_at}, got {got:?}");
    }

    let a = acl::get_acl_entity_for_username(b"book", b"alice");
    if !a.as_ref().is_some_and(|a| a.can(Permission::Read)) {
        panic!("expected the ACLs of the backup, got {a:?}");
    }
}

#[test]
fn restore_skips_expired_keys() {
    let mut folders = Folders::default();
    folders.setup("");
    storage::put_key_value_with_expiration(b"app:old", b"x", Time::now().unix() + 3600)
        .expect("put");
    storage::put_key_value(b"app:new", b"y").expect("put");
    let archive = rebuild(&create(), |name, data| {
        if name != backup::KV_PATH {
            return data;
        }

        let text = String::from_utf8(data).expect("utf8");
        let mut out = Vec::new();
        for line in text.trim().split('\n') {
            let mut e = KvEntry::default();
            let _ = json::unmarshal(line.as_bytes(), &mut e);
            if e.expires_at > 0 {
                e.expires_at = Time::now().unix() - 10;
            }

            out.push(String::from_utf8(json::marshal(&e).expect("marshal")).expect("utf8"));
        }

        (out.join("\n") + "\n").into_bytes()
    });

    folders.setup("");
    let report = restore(&archive);
    if report.kv_keys != 1 || report.expired_kv_keys != 1 {
        panic!("expected one restored and one expired key, got {report:?}");
    }

    if storage::get_by_key(b"app:old").is_ok() {
        panic!("an expired key must not be restored");
    }
}

#[test]
fn restore_leaves_the_data_untouched_on_an_invalid_archive() {
    let mut folders = Folders::default();
    folders.setup("");
    api::write_entity(b"book", &mut object(r#"{"id":"b1","title":"Dune"}"#));
    let archive = create();

    folders.setup("");
    api::write_entity(b"keep", &mut object(r#"{"id":"k1"}"#));
    let key_before = security::create_key_file_or_get_key().expect("key");

    let invalid = rebuild(&archive, |name, data| {
        if name == backup::entity_path(b"book") {
            return b"{\"id\":\"b1\"}\n{\"id\":\"b1\"}\n".to_vec();
        }

        data
    });

    let err = backup::restore(std::io::Cursor::new(invalid))
        .expect_err("expected an archive error, got a report");
    if err.downcast_ref::<ArchiveError>().is_none() {
        panic!("expected an archive error, got {err}");
    }

    if api::read_entity_by_id(b"keep", b"k1").is_none()
        || api::read_entity_by_id(b"book", b"b1").is_some()
    {
        panic!("a refused archive must leave the data untouched");
    }

    if security::create_key_file_or_get_key().expect("key") != key_before {
        panic!("a refused archive must leave the users key untouched");
    }
}

#[test]
fn write_file_names_the_archive_after_its_date() {
    let mut folders = Folders::default();
    folders.setup("");
    let folder = folders.temp();
    let now = backup_time();

    let (path, manifest) = match backup::write_file(&folder, now.clone()) {
        Ok(written) => written,
        Err(err) => panic!("backup failed: {err}"),
    };

    if path != folder.join("elysiandb-backup-20261101T091400Z.tar.gz")
        || manifest.created_at != "2026-11-01T09:14:00Z"
    {
        panic!("unexpected backup {} {manifest:?}", path.display());
    }

    match std::fs::metadata(&path) {
        Ok(info) if info.permissions().mode() & 0o777 == 0o600 => {}
        other => panic!("expected a private archive, got {other:?}"),
    }

    if backup::write_file(&folder, now).is_ok() {
        panic!("an existing backup must not be overwritten");
    }

    let files: Vec<_> = std::fs::read_dir(&folder)
        .expect("read folder")
        .flatten()
        .map(|e| e.file_name())
        .collect();
    if files.len() != 1 {
        panic!("expected no temporary file left, got {files:?}");
    }
}
