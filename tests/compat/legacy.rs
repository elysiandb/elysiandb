use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use elysiandb::acl;
use elysiandb::boot;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Base64Bytes};
use elysiandb::hook;
use elysiandb::security;

use super::corpus::{CorpusServer, Engine, corpus_config, register_packages, serve_routes};
use super::fixture::{
    FixtureRequest, READER_ACL, READER_USER, fixture_load_requests, hook_requests, read_requests,
};
use super::golden::{
    GoldenEntry, LEGACY_FIXTURES_DIR, LEGACY_GOLDEN_PATH, differs, expected_body,
    intentional_change, read_legacy_golden, skip, testdata, truncate,
};
use super::tempdir::TempDir;

const LEGACY_VARIANTS: [&str; 6] = [
    "full",
    "pending",
    "kv-only",
    "empty",
    "inconsistent",
    "failed-log",
];

fn write_file(path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    f.write_all(data)
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(src)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let target = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o755)
                .create(&target)?;
            copy_tree(&e.path(), &target)?;
            continue;
        }

        let data = std::fs::read(e.path())?;
        write_file(&target, &data, 0o600)?;
    }

    Ok(())
}

pub fn copy_folder(src: &Path) -> TempDir {
    let dst = TempDir::new();
    if let Err(e) = copy_tree(src, dst.path()) {
        panic!("cannot copy {}: {e}", src.display());
    }

    dst
}

pub fn start_server_on_folder(folder: &Path, crash_recovery: bool) -> CorpusServer {
    register_packages();

    let mut cfg = corpus_config(&folder.to_string_lossy());
    cfg.store.crash_recovery.enabled = crash_recovery;
    cfg.store.crash_recovery.max_log_mb = 100;
    globals::set_config(Some(Arc::new(cfg)));
    security::reset_login_attempts();
    boot::init_db();
    hook::init_hooks();

    if let Err(e) = security::init_admin_user_if_not_exists() {
        panic!("cannot create the default admin: {e}");
    }

    let _ = acl::init_acl();

    serve_routes(None).0
}

pub fn legacy_read_requests() -> Vec<FixtureRequest> {
    let mut reqs: Vec<FixtureRequest> = [
        "/kv/session:1",
        "/kv/derived:total",
        "/kv/long",
        "/kv/pending:key",
        "/kv/mget?keys=session:1,long,derived:total,pending:key",
        "/api/hook/budget_month",
        "/api/acl/reader/note",
        "/api/note",
        "/api/pending",
        "/api/pending/count",
    ]
    .iter()
    .map(|p| FixtureRequest::new("GET", p, ""))
    .collect();
    reqs.extend(read_requests());

    reqs
}

#[test]
fn legacy_folder_corpus() {
    let _engine = Engine;
    let record = std::env::var("ELYSIAN_RECORD_GOLDEN").as_deref() == Ok("1");

    let mut folders = Vec::new();
    let mut results: BTreeMap<String, Vec<GoldenEntry>> = BTreeMap::new();
    for variant in LEGACY_VARIANTS {
        let folder = copy_folder(&testdata(LEGACY_FIXTURES_DIR).join(variant));
        let mut s = start_server_on_folder(folder.path(), variant == "pending");
        let mut entries = Vec::new();
        for r in legacy_read_requests() {
            entries.push(s.run(&r));
        }
        results.insert(variant.to_string(), entries);
        folders.push(folder);
    }

    if record {
        let mut data = json::marshal_indent(&results, "", "  ").unwrap_or_default();
        data.push(b'\n');
        std::fs::write(testdata(LEGACY_GOLDEN_PATH), data).unwrap_or_else(|e| panic!("{e}"));
        return;
    }

    let golden = read_legacy_golden();
    let none = Vec::new();

    let mut errors = Vec::new();
    for variant in LEGACY_VARIANTS {
        let want = golden.get(variant).unwrap_or(&none);
        let got = results.get(variant).unwrap_or(&none);
        if want.len() != got.len() {
            panic!(
                "{variant}: {} requests, golden has {}",
                got.len(),
                want.len()
            );
        }

        let mut failures = 0;
        for i in 0..want.len() {
            if intentional_change(&want[i].request) {
                continue;
            }

            if differs(&got[i], &want[i], false) {
                failures += 1;
                if failures <= 10 {
                    errors.push(format!(
                        "{variant} #{i} {} {}\nwant {} {}\ngot  {} {}",
                        want[i].request.method,
                        want[i].request.path,
                        want[i].status,
                        truncate(expected_body(&want[i], false)),
                        got[i].status,
                        truncate(&got[i].body)
                    ));
                }
            }
        }
    }

    if !errors.is_empty() {
        panic!("{}", errors.join("\n"));
    }
}

#[test]
fn record_legacy_fixtures() {
    if std::env::var("ELYSIAN_RECORD_FIXTURES").as_deref() != Ok("1") {
        skip("set ELYSIAN_RECORD_FIXTURES=1 to write the 0.2.4 data folders");
        return;
    }

    let _engine = Engine;
    for variant in LEGACY_VARIANTS {
        let target = testdata(LEGACY_FIXTURES_DIR).join(variant);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(&target)
            .unwrap_or_else(|e| panic!("{e}"));

        let full = testdata(LEGACY_FIXTURES_DIR).join("full");
        match variant {
            "inconsistent" => derive_inconsistent_variant(&full, &target),
            "failed-log" => derive_failed_log_variant(&full, &target),
            _ => write_legacy_variant(variant, &target),
        }
    }
}

fn copy_files(src: &Path, dst: &Path) {
    let mut names: Vec<PathBuf> = std::fs::read_dir(src)
        .unwrap_or_else(|e| panic!("{e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    names.sort();

    for path in names {
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{e}"));
        let Some(name) = path.file_name() else {
            continue;
        };
        write_file(&dst.join(name), &data, 0o644).unwrap_or_else(|e| panic!("{e}"));
    }
}

fn without(list: &[u8], dropped: &[&[u8]]) -> Vec<Vec<u8>> {
    list.split(|&c| c == b'\n')
        .filter(|item| !dropped.contains(item))
        .map(<[u8]>::to_vec)
        .collect()
}

fn derive_inconsistent_variant(src: &Path, dst: &Path) {
    copy_files(src, dst);

    let path = dst.join("elysiandb.json");
    let raw = std::fs::read(&path).unwrap_or_else(|e| panic!("{e}"));

    let mut kv: BTreeMap<String, Base64Bytes> = BTreeMap::new();
    json::unmarshal(&raw, &mut kv).unwrap_or_else(|e| panic!("{e}"));

    let key = "api:entity:book:internal:index:id";
    let ids = kv.get(key).map(|v| v.0.clone()).unwrap_or_default();
    let mut kept = without(&ids, &[b"b03", b"b11"]);
    kept.push(b"ghost".to_vec());
    kv.insert(key.to_string(), Base64Bytes(kept.join(&b'\n')));

    let types = kv
        .get("api:entity:types:list")
        .map(|v| v.0.clone())
        .unwrap_or_default();
    let kept_types = without(&types, &[b"review"]);
    kv.insert(
        "api:entity:types:list".to_string(),
        Base64Bytes(kept_types.join(&b'\n')),
    );

    let mut out = json::marshal(&kv).unwrap_or_default();
    out.push(b'\n');
    write_file(&path, &out, 0o644).unwrap_or_else(|e| panic!("{e}"));
}

fn derive_failed_log_variant(src: &Path, dst: &Path) {
    copy_files(src, dst);

    write_file(
        &dst.join("elysiandb.json.recovery.log.failed"),
        b"{not json\n",
        0o644,
    )
    .unwrap_or_else(|e| panic!("{e}"));
}

fn write_legacy_variant(variant: &str, target: &Path) {
    let folder = TempDir::new();
    let mut s = start_server_on_folder(folder.path(), variant == "pending");

    if variant == "full" || variant == "pending" {
        let mut reqs = fixture_load_requests();
        reqs.extend(hook_requests());
        reqs.push(FixtureRequest::new(
            "POST",
            "/api/security/user",
            READER_USER,
        ));
        for r in reqs {
            s.run(&r);
        }
        s.run(&FixtureRequest::new(
            "POST",
            "/api/note",
            r#"{"id":"n1","text":"admin note"}"#,
        ));
        s.run(&FixtureRequest::new(
            "PUT",
            "/api/acl/reader/note",
            READER_ACL,
        ));
    }

    if variant != "empty" {
        for r in [
            FixtureRequest::new("PUT", "/kv/session:1", "alpha"),
            FixtureRequest::new("PUT", "/kv/derived:total", "42"),
            FixtureRequest::new("PUT", "/kv/long?ttl=900000000", "kept for years"),
            FixtureRequest::new("PUT", "/kv/soon?ttl=1", "expires"),
        ] {
            s.run(&r);
        }
    }

    s.run(&FixtureRequest::new("POST", "/save", ""));

    if variant == "pending" {
        for r in [
            FixtureRequest::new("PUT", "/kv/pending:key", "written after the flush"),
            FixtureRequest::new(
                "POST",
                "/api/pending",
                r#"[{"id":"x1","v":1},{"id":"x2","v":2}]"#,
            ),
            FixtureRequest::new("PUT", "/api/book/b07", r#"{"status":"pending-update"}"#),
            FixtureRequest::new("DELETE", "/api/book/b08", ""),
        ] {
            s.run(&r);
        }
    }

    std::thread::sleep(Duration::from_millis(1100));

    let mut entries: Vec<_> = std::fs::read_dir(folder.path())
        .unwrap_or_else(|e| panic!("{e}"))
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for e in entries {
        let name = e.file_name();
        if e.file_type().is_ok_and(|t| t.is_dir()) || name.to_string_lossy().ends_with(".tmp") {
            continue;
        }

        let data = std::fs::read(e.path()).unwrap_or_else(|e| panic!("{e}"));
        write_file(&target.join(&name), &data, 0o644).unwrap_or_else(|e| panic!("{e}"));
    }
}
