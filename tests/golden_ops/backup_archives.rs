use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::sync::Arc;

use std::io::Read;

use std::cell::Cell;
use std::net::SocketAddr;
use std::time::Duration;

use elysiandb::archive::{gzip, tar};
use elysiandb::backup::{self, ArchiveError, Report};
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::base64;
use elysiandb::gocompat::json::{self, Value};
use elysiandb::gocompat::time::Time;
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::httprouter::Router;
use elysiandb::routing;
use elysiandb::storage;
use elysiandb::transport::http::api as http_api;
use elysiandb::transport::tcp::tcp_routing::{self, Conn};

use super::archives::{archive, gunzip, gzip_once};
use super::common::{read_jsonl, str_field};
use super::tar_archives::expected_detail;
use super::tempdir::TempDir;

const EXPECTED_ROWS: usize = 87;
const ARCHIVE_LAYER_ROWS: usize = 7;
const JSON_ROWS: usize = 16;
const ACCEPTED_ROWS: usize = 11;
const BACKUP_ROWS: usize = 80;
const RESTORE_ROWS: usize = 19;
const REBACKUP_ROWS: usize = 8;
const RECIPE_ROWS: usize = 27;

const GO_GZIP: &str = "\u{27e8}go:gzip\u{27e9}";
const GO_TAR: &str = "\u{27e8}go:tar\u{27e9}";
const GO_JSON: &str = "\u{27e8}go:json\u{27e9}";

const JSON_DETAILS: [(&str, &str); JSON_ROWS] = [
    (
        "invalid/manifest-truncated-json.tar.gz",
        "unexpected end of input",
    ),
    (
        "invalid/manifest-trailing-data.tar.gz",
        "byte 4: unexpected character 'x'",
    ),
    (
        "invalid/manifest-float-version.tar.gz",
        "\"format_version\" must be an integer, got a number",
    ),
    (
        "invalid/manifest-string-version.tar.gz",
        "\"format_version\" must be an integer, got a string",
    ),
    (
        "invalid/manifest-array.tar.gz",
        "the value must be an object, got an array",
    ),
    (
        "invalid/manifest-huge-number.tar.gz",
        "\"format_version\": 1e400 is out of range",
    ),
    (
        "invalid/document-truncated-json.tar.gz",
        "invalid JSON: unexpected end of input",
    ),
    (
        "invalid/document-garbage.tar.gz",
        "invalid JSON at byte 1: unexpected character 'x'",
    ),
    (
        "invalid/document-array.tar.gz",
        "invalid JSON: the value must be an object, got an array",
    ),
    (
        "invalid/document-trailing-garbage.tar.gz",
        "invalid JSON at byte 13: unexpected character ']'",
    ),
    (
        "invalid/kv-unknown-field.tar.gz",
        "invalid JSON: unknown field \"x\"",
    ),
    (
        "invalid/kv-key-number.tar.gz",
        "invalid JSON: \"key\" must be a string, got a number",
    ),
    (
        "invalid/kv-expiry-out-of-range.tar.gz",
        "invalid JSON: \"expires_at\": 1e400 is out of range",
    ),
    (
        "invalid/kv-expiry-float.tar.gz",
        "invalid JSON: \"expires_at\" must be an integer, got a number",
    ),
    (
        "invalid/kv-garbage.tar.gz",
        "invalid JSON at byte 1: unexpected character 'x'",
    ),
    (
        "invalid/kv-array.tar.gz",
        "invalid JSON: the value must be an object, got an array",
    ),
];

pub(super) struct Store {
    _dir: TempDir,
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

impl Store {
    pub(super) fn path(&self) -> &std::path::Path {
        self._dir.path()
    }
}

pub(super) fn open_store() -> Store {
    let _ = storage::close();
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().join("data").to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.api.schema.enabled = true;
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();

    Store { _dir: dir }
}

fn reopen() {
    let _ = storage::close();
    storage::load_db();
}

fn octal(field: &[u8]) -> usize {
    let text: String = field
        .iter()
        .filter(|&&c| c != 0 && c != b' ')
        .map(|&c| c as char)
        .collect();
    usize::from_str_radix(&text, 8).unwrap_or(0)
}

fn mask_value(data: &[u8], key: &str) -> Vec<u8> {
    let marker = format!("\"{key}\": \"");
    let Some(start) = data
        .windows(marker.len())
        .position(|w| w == marker.as_bytes())
    else {
        return data.to_vec();
    };
    let from = start + marker.len();
    let Some(len) = data[from..].iter().position(|&c| c == b'"') else {
        return data.to_vec();
    };

    let mut out = data[..from].to_vec();
    out.extend_from_slice(&data[from + len..]);
    out
}

pub(super) fn normalized(tar: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(tar.len());
    let mut off = 0;
    let mut pax_size: Option<usize> = None;
    while off + 512 <= tar.len() {
        let mut header = tar[off..off + 512].to_vec();
        if header.iter().all(|&c| c == 0) {
            out.extend_from_slice(&tar[off..]);
            return out;
        }

        let flag = header[156];
        let mut size = octal(&header[124..136]);
        if flag != b'x'
            && let Some(s) = pax_size.take()
        {
            size = s;
        }

        let name_end = header[..100].iter().position(|&c| c == 0).unwrap_or(100);
        let name = header[..name_end].to_vec();
        header[136..156].fill(0);

        let end = (off + 512 + size).min(tar.len());
        let mut data = tar[off + 512..end].to_vec();
        if flag == b'x' {
            for record in String::from_utf8_lossy(&data).split('\n') {
                if let Some(i) = record.find(" size=") {
                    pax_size = record[i + 6..].parse().ok();
                }
            }
        }

        if name == b"manifest.json" {
            header[124..136].fill(0);
            data = mask_value(&mask_value(&data, "created_at"), "elysiandb_version");
        }

        out.extend_from_slice(&header);
        out.extend_from_slice(&data);
        out.resize(out.len() + (512 - data.len() % 512) % 512, 0);
        off += 512 + size.div_ceil(512) * 512;
    }

    out.extend_from_slice(&tar[off.min(tar.len())..]);
    out
}

fn expected_reason(row: &Value, data: &[u8]) -> Option<String> {
    let reason = row.get("reason").and_then(Value::as_str)?;
    let file = str_field(row, "file");

    if reason.contains(GO_GZIP) || reason.contains(GO_TAR) {
        let detail = expected_detail(row, data, reason);
        return Some(reason.replace(GO_GZIP, &detail).replace(GO_TAR, &detail));
    }

    if reason.contains(GO_JSON) {
        let (_, detail) = JSON_DETAILS
            .iter()
            .find(|(f, _)| *f == file)
            .unwrap_or_else(|| panic!("no D27 text for {file}"));
        return Some(reason.replace(GO_JSON, detail));
    }

    if let Some(version) = row.get("version").and_then(Value::as_str) {
        return Some(reason.replace(
            &format!("ElysianDB {version}"),
            &format!("ElysianDB {}", globals::VERSION),
        ));
    }

    Some(reason.to_string())
}

#[test]
fn every_archive_is_verified_with_the_reason_of_go() {
    let mut rows = 0;
    let mut archive_layer = 0;
    let mut json_rows = 0;
    let mut accepted = 0;
    let mut matched_json = BTreeSet::new();
    for row in &read_jsonl("ops/archives/expected.jsonl") {
        let file = str_field(row, "file");
        let data = archive(file);
        let got = backup::verify(data.as_slice());

        match expected_reason(row, &data) {
            None => {
                let manifest = got.unwrap_or_else(|e| panic!("{file}: {e}"));
                assert_eq!(manifest.format, backup::FORMAT, "{file}");
                accepted += 1;
            }
            Some(want) => {
                let err = got.expect_err(file);
                assert!(err.downcast_ref::<ArchiveError>().is_some(), "{file}");
                assert_eq!(err.to_string(), want, "{file}");
                let reason = str_field(row, "reason");
                if reason.contains(GO_GZIP) || reason.contains(GO_TAR) {
                    archive_layer += 1;
                }
                if reason.contains(GO_JSON) {
                    json_rows += 1;
                    matched_json.insert(file.to_string());
                }
            }
        }
        rows += 1;
    }

    assert_eq!(rows, EXPECTED_ROWS);
    assert_eq!(archive_layer, ARCHIVE_LAYER_ROWS);
    assert_eq!(json_rows, JSON_ROWS);
    assert_eq!(matched_json.len(), JSON_ROWS);
    assert_eq!(accepted, ACCEPTED_ROWS);
    assert_eq!(rows - archive_layer, BACKUP_ROWS);
}

fn report_lines(report: &Report) -> Vec<String> {
    let mut lines = Vec::new();
    for (entity, count) in &report.entities {
        lines.push(format!(
            "  {}: {count} documents",
            String::from_utf8_lossy(entity)
        ));
    }
    lines.push(format!("  schemas: {}", report.schemas));
    lines.push(format!(
        "  KV keys: {} ({} expired and skipped)",
        report.kv_keys, report.expired_kv_keys
    ));
    lines
}

fn go_report_lines(stdout: &str) -> Vec<String> {
    let mut lines = stdout.lines();
    for line in lines.by_ref() {
        if line.contains("Restore completed.") {
            break;
        }
    }
    lines.map(str::to_string).collect()
}

fn rebackup() -> Vec<u8> {
    reopen();
    let mut buf = Vec::new();
    backup::create(&mut buf, Time::now().utc()).expect("rebackup");
    gunzip(&buf).expect("rebackup gunzip")
}

#[test]
fn go_archives_restore_like_go_and_back_up_again_like_go() {
    let mut rows = 0;
    let mut rebackups = 0;
    let mut differing: Vec<(String, Vec<String>)> = Vec::new();
    for row in &read_jsonl("ops/archives/restore.jsonl") {
        let file = str_field(row, "file");
        assert_eq!(row.get("exit").and_then(Value::as_i64), Some(0), "{file}");
        let data = archive(file);

        let _store = open_store();
        let report =
            backup::restore(Cursor::new(data.clone())).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(
            report_lines(&report),
            go_report_lines(str_field(row, "stdout")),
            "{file}"
        );
        assert!(!backup::restore_was_interrupted(), "{file}");

        let again_tar = rebackup();
        let again = normalized(&again_tar);
        let equal = gunzip(&data)
            .map(|original| normalized(&original) == again)
            .unwrap_or(false);
        assert_eq!(
            Some(equal),
            row.get("rebackup_equal").and_then(Value::as_bool),
            "{file}"
        );

        if let Some(rel) = row.get("rebackup").and_then(Value::as_str) {
            assert!(normalized(&archive(rel)) == again, "{file}: {rel}");
            rebackups += 1;
        } else if !equal {
            let original = gunzip_partial(&data);
            differing.push((file.to_string(), differences(&original, &again_tar)));
            let go_keys = tar_entries(&original)
                .remove(backup::KV_PATH)
                .expect("go keys");
            if let Some(want) = expected_keys(file, &go_keys) {
                let ours = tar_entries(&again_tar)
                    .remove(backup::KV_PATH)
                    .expect("keys");
                assert!(ours == want, "{file}: {}", String::from_utf8_lossy(&ours));
            }
            if let Some((_, same_as)) = SAME_AS_GO.iter().find(|(f, _)| *f == file) {
                assert!(normalized(&archive(same_as)) == again, "{file}: {same_as}");
            }
        }
        rows += 1;
    }

    assert_eq!(rows, RESTORE_ROWS);
    assert_eq!(rebackups, REBACKUP_ROWS);
    let want: Vec<(String, Vec<String>)> = REBACKUP_DIFFERENCES
        .iter()
        .map(|(f, d)| (f.to_string(), d.iter().map(|s| s.to_string()).collect()))
        .collect();
    assert_eq!(differing, want);
}

const SAME_AS_GO: [(&str, &str); 1] = [("seed/seed.tar.gz", "seed-only/backup.tar")];

const KV_AND_MANIFEST: &[&str] = &["kv/keys.jsonl", "manifest.json"];
const NEW_ADMIN: &[&str] = &[
    "entities/_elysiandb_core_user.jsonl",
    "manifest.json",
    "schemas/_elysiandb_core_user.json",
];

const REBACKUP_DIFFERENCES: [(&str, &[&str]); 7] = [
    ("seed/seed.tar.gz", KV_AND_MANIFEST),
    ("api/periodic.tar.gz", KV_AND_MANIFEST),
    ("api/hot.tar.gz", KV_AND_MANIFEST),
    ("api/backup.tar.gz", KV_AND_MANIFEST),
    ("valid/gzip-trailer-missing.tar.gz", NEW_ADMIN),
    ("valid/gzip-bad-crc.tar.gz", NEW_ADMIN),
    ("valid/gzip-bad-size.tar.gz", NEW_ADMIN),
];

fn expected_keys(file: &str, go: &[u8]) -> Option<Vec<u8>> {
    let text = String::from_utf8(go.to_vec()).expect("utf8 keys");
    if file == "seed/seed.tar.gz" {
        let kept: String = text
            .lines()
            .filter(|l| !l.starts_with(r#"{"key":"old","#))
            .map(|l| format!("{l}\n"))
            .collect();
        return Some(kept.into_bytes());
    }
    if file.starts_with("api/") {
        let escape = format!("{}ufffd", '\\');
        assert!(text.contains(&escape), "{file}");
        return Some(text.replace(&escape, "\u{fffd}").into_bytes());
    }
    None
}

fn gunzip_partial(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if let Ok(mut r) = gzip::new_reader(data) {
        let _ = r.read_to_end(&mut out);
    }
    out
}

fn tar_entries(data: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut tr = tar::Reader::new(data);
    while let Ok(Some(header)) = tr.next() {
        let mut content = Vec::new();
        let _ = tr.read_to_end(&mut content);
        let name = String::from_utf8_lossy(&header.name).into_owned();
        if name == backup::MANIFEST_PATH {
            content = mask_value(&mask_value(&content, "created_at"), "elysiandb_version");
        }
        out.insert(name, content);
    }
    out
}

fn differences(go: &[u8], ours: &[u8]) -> Vec<String> {
    let go = tar_entries(go);
    let ours = tar_entries(ours);
    let names: BTreeSet<&String> = go.keys().chain(ours.keys()).collect();
    names
        .into_iter()
        .filter(|name| go.get(*name) != ours.get(*name))
        .cloned()
        .collect()
}

fn go_tars_of(dataset: &str) -> Vec<(String, Vec<u8>)> {
    read_jsonl("ops/archives/archives.jsonl")
        .iter()
        .filter(|row| str_field(row, "dataset") == dataset)
        .map(|row| {
            let kind = str_field(row, "kind").to_string();
            let tar = archive(str_field(row, "tar"));
            let gz = archive(str_field(row, "file"));
            assert!(gunzip(&gz).unwrap() == tar, "{dataset}/{kind}");
            (kind, tar)
        })
        .collect()
}

struct RecipeConn {
    closed: Cell<bool>,
}

impl Conn for RecipeConn {
    fn close(&self) {
        self.closed.set(true);
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        Some(SocketAddr::from(([127, 0, 0, 1], 40000)))
    }
}

fn http_call(router: &Router, method: &str, path: &str, body: &[u8]) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(path);
    req.header.set_method(method);
    if !body.is_empty() {
        req.header.set_content_type("application/json");
        req.set_body(body);
    }
    let mut ctx = RequestCtx::new();
    ctx.init(&req, Some(SocketAddr::from(([127, 0, 0, 1], 40001))));
    router.handler(&mut ctx);
    ctx
}

fn bytes_of(step: &Value, field: &str) -> Vec<u8> {
    match step.get(field).and_then(Value::as_str) {
        Some(text) => text.as_bytes().to_vec(),
        None => base64::STD_ENCODING
            .decode_string(str_field(step, &format!("{field}_b64")).as_bytes())
            .unwrap_or_else(|e| panic!("{field}_b64: {e:?}")),
    }
}

fn status_of(step: &Value) -> u16 {
    step.get("status")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("no status in {step:?}")) as u16
}

fn run_step(dataset: &str, router: &Router, step: &Value) {
    match str_field(step, "op") {
        "restore" => {
            let data = archive(str_field(step, "file"));
            backup::restore(Cursor::new(data)).unwrap_or_else(|e| panic!("{dataset}: {e}"));
        }
        "http" => {
            let ctx = http_call(
                router,
                str_field(step, "method"),
                str_field(step, "path"),
                str_field(step, "body").as_bytes(),
            );
            assert_eq!(
                ctx.response.status_code(),
                status_of(step),
                "{dataset}: {}",
                str_field(step, "path")
            );
        }
        "http_repeat" => {
            let count = step.get("count").and_then(Value::as_i64).unwrap_or(0);
            let width = step.get("id_width").and_then(Value::as_i64).unwrap_or(0) as usize;
            let items = step.get("items").and_then(Value::as_i64).unwrap_or(0) as usize;
            let mut statuses: BTreeMap<String, Value> = BTreeMap::new();
            for n in 0..count {
                let body = format!(
                    "{}{}{n:0width$}{}{}{}",
                    str_field(step, "body_head"),
                    str_field(step, "id_prefix"),
                    str_field(step, "body_mid"),
                    str_field(step, "item").repeat(items),
                    str_field(step, "body_tail"),
                );
                let ctx = http_call(
                    router,
                    str_field(step, "method"),
                    str_field(step, "path"),
                    body.as_bytes(),
                );
                let key = ctx.response.status_code().to_string();
                let seen = match statuses.get(&key) {
                    Some(Value::Int(v)) => *v,
                    _ => 0,
                };
                statuses.insert(key, Value::Int(seen + 1));
            }
            assert_eq!(
                json::marshal(&statuses).expect("statuses"),
                json::marshal(step.get("statuses").expect("statuses")).expect("statuses"),
                "{dataset}: statuses"
            );
        }
        "tcp" => {
            let conn = RecipeConn {
                closed: Cell::new(false),
            };
            let mut session = tcp_routing::new_session();
            let mut reply = tcp_routing::route_line(&bytes_of(step, "send"), &conn, &mut session);
            reply.push(b'\n');
            assert_eq!(
                String::from_utf8_lossy(&reply),
                String::from_utf8_lossy(&bytes_of(step, "reply")),
                "{dataset}: tcp step"
            );
        }
        "sleep" => {
            let ms = step.get("ms").and_then(Value::as_i64).unwrap_or(0) as u64;
            std::thread::sleep(Duration::from_millis(ms));
        }
        other => panic!("{dataset}: unknown op {other}"),
    }
}

fn hot_backup(router: &Router) -> (String, Vec<u8>) {
    let mut ctx = http_call(router, "GET", "/api/backup", b"");
    assert_eq!(ctx.response.status_code(), 200);
    assert_eq!(ctx.response.header.content_type(), b"application/gzip");
    let name =
        String::from_utf8_lossy(ctx.response.header.peek("Content-Disposition")).into_owned();
    let body = ctx.response.body().to_vec();
    (name, body)
}

fn served_name(name: &str) -> String {
    let Some(start) = name.find("elysiandb-backup-") else {
        return name.to_string();
    };
    let stamp = start + "elysiandb-backup-".len();
    let Some(end) = name[stamp..].find(".tar.gz") else {
        return name.to_string();
    };
    format!("{}<STAMP>{}", &name[..stamp], &name[stamp + end..])
}

fn replay(dataset: &str, steps: &[&Value]) -> Vec<(String, Vec<u8>)> {
    let store = open_store();
    elysiandb::security::register();
    let mut router = Router::new();
    routing::register_routes(&mut router);
    for step in steps {
        run_step(dataset, &router, step);
    }

    let mut built = Vec::new();

    let (name, hot) = hot_backup(&router);
    for row in read_jsonl("ops/archives/archives.jsonl") {
        if str_field(&row, "dataset") == dataset && str_field(&row, "kind").starts_with("hot") {
            assert_eq!(
                served_name(&name),
                str_field(&row, "served_name"),
                "{dataset}: served name"
            );
        }
    }
    built.push(("hot".to_string(), hot));

    let periodic = store.path().join("periodic");
    backup::run_periodic(&periodic.to_string_lossy(), 3);
    let files: Vec<_> = std::fs::read_dir(&periodic)
        .expect("periodic folder")
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(files.len(), 1, "{dataset}");
    built.push((
        "periodic".to_string(),
        std::fs::read(&files[0]).expect("periodic archive"),
    ));

    reopen();
    let out = store.path().join("out");
    std::fs::create_dir(&out).expect("out");
    let (path, _) = backup::write_file(&out, Time::now().utc()).expect("backup");
    built.push(("backup".to_string(), std::fs::read(path).expect("archive")));

    built
}

#[test]
fn recipe_data_sets_rebuild_the_tar_of_go() {
    let recipe = read_jsonl("ops/archives/recipe.jsonl");
    assert_eq!(recipe.len(), RECIPE_ROWS);

    let mut steps: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for row in &recipe {
        steps
            .entry(str_field(row, "dataset").to_string())
            .or_default()
            .push(row);
    }

    let mut replayed = 0;
    for (dataset, rows) in &steps {
        assert_eq!(str_field(rows[0], "op"), "restore", "{dataset}");

        let go = go_tars_of(dataset);
        assert!(!go.is_empty(), "{dataset}");
        let reference = normalized(&go[0].1);
        for (kind, tar) in &go {
            assert!(normalized(tar) == reference, "{dataset}/{kind}");
        }

        for (kind, gz) in replay(dataset, rows) {
            let tar = gunzip(&gz).unwrap_or_else(|e| panic!("{dataset}/{kind}: {e}"));
            assert!(normalized(&tar) == reference, "{dataset}/{kind}");
            assert_eq!(&gz[..10], b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x04\xff");
            if kind != "hot" {
                assert!(gzip_once(&tar) == gz, "{dataset}/{kind}");
            }
        }
        replayed += 1;
    }

    assert_eq!(replayed, 3);
    assert_eq!(steps.len(), replayed);
}

#[test]
fn every_archive_is_answered_over_http_with_the_reason_of_go() {
    let mut rows = 0;
    let mut accepted = 0;
    for row in &read_jsonl("ops/archives/expected.jsonl") {
        let _store = open_store();
        let file = str_field(row, "file");
        let data = archive(file);

        let mut req = Request::new();
        req.set_request_uri("/api/restore");
        req.header.set_method("POST");
        req.header.set_content_type("application/gzip");
        req.set_body(&data);
        let mut ctx = RequestCtx::new();
        ctx.init(&req, None);
        http_api::restore_controller(&mut ctx);

        let body = String::from_utf8_lossy(ctx.response.body_bytes()).into_owned();
        let reason = match data.is_empty() {
            true => Some("the request body must be a backup archive".to_string()),
            false => expected_reason(row, &data),
        };
        match reason {
            None => {
                assert_eq!(ctx.response.status_code(), 200, "{file}: {body}");
                assert!(
                    body.contains("\"status\":\"restore completed\""),
                    "{file}: {body}"
                );
                accepted += 1;
            }
            Some(want) => {
                let mut m = BTreeMap::new();
                m.insert("error".to_string(), want);
                let want =
                    String::from_utf8(json::marshal(&m).expect("error body")).expect("utf-8");
                assert_eq!(ctx.response.status_code(), 400, "{file}: {body}");
                assert_eq!(body, want, "{file}");
            }
        }
        assert_eq!(
            ctx.response.header.content_type(),
            b"application/json",
            "{file}"
        );
        rows += 1;
    }

    assert_eq!(rows, EXPECTED_ROWS);
    assert_eq!(accepted, ACCEPTED_ROWS);
}
