use std::collections::BTreeMap;
use std::io::Read;
use std::process::Command;

use elysiandb::archive::{gzip, tar};
use elysiandb::backup;
use elysiandb::gocompat::json::{self, Value};
use elysiandb::gocompat::time::Time;

use super::corpus::{CorpusServer, Engine, shutdown_engine};
use super::golden::{
    ARCHIVES_DIR, GoldenEntry, LEGACY_FIXTURES_DIR, differs, expected_body, golden_full,
    intentional_change, skip, testdata, truncate,
};
use super::legacy::{copy_folder, legacy_read_requests, start_server_on_folder};
use super::tempdir::TempDir;

fn compare_with_golden(
    errors: &mut Vec<String>,
    label: &str,
    s: &mut CorpusServer,
    want: &[GoldenEntry],
) {
    for (i, r) in legacy_read_requests().iter().enumerate() {
        if intentional_change(&want[i].request) {
            continue;
        }

        let got = s.run(r);
        if differs(&got, &want[i], false) {
            errors.push(format!(
                "{label} #{i} {} {}\nwant {} {}\ngot  {} {}",
                r.method,
                r.path,
                want[i].status,
                truncate(expected_body(&want[i], false)),
                got.status,
                truncate(&got.body)
            ));
        }
    }
}

#[test]
fn archives_of_earlier_versions_are_restored_identically() {
    let _engine = Engine;
    let want = golden_full();

    let mut errors = Vec::new();
    for name in ["v0.2.4.tar.gz", "c104f4a.tar.gz"] {
        let folder = TempDir::new();
        let server = start_server_on_folder(folder.path(), true);

        let f = std::fs::File::open(testdata(ARCHIVES_DIR).join(name))
            .unwrap_or_else(|e| panic!("{e}"));

        if let Err(e) = backup::restore(f) {
            panic!("{name}: {e}");
        }
        shutdown_engine();
        drop(server);

        for round in 0..2 {
            let mut s = start_server_on_folder(folder.path(), true);
            compare_with_golden(&mut errors, &format!("{name} round {round}"), &mut s, &want);
            shutdown_engine();
        }
    }

    if !errors.is_empty() {
        panic!("{}", errors.join("\n"));
    }
}

fn read_archive(data: &[u8]) -> (Vec<String>, BTreeMap<String, Vec<u8>>) {
    let gz = gzip::new_reader(data).unwrap_or_else(|e| panic!("{e}"));

    let mut names = Vec::new();
    let mut files = BTreeMap::new();
    let mut tr = tar::Reader::new(gz);
    loop {
        let h = match tr.next() {
            Ok(Some(h)) => h,
            Ok(None) => return (names, files),
            Err(e) => panic!("{e}"),
        };

        let mut content = Vec::new();
        tr.read_to_end(&mut content)
            .unwrap_or_else(|e| panic!("{e}"));

        let name = String::from_utf8_lossy(&h.name).into_owned();
        names.push(name.clone());
        files.insert(name, content);
    }
}

fn manifest_without_varying_fields(files: &BTreeMap<String, Vec<u8>>) -> Value {
    let raw = files.get("manifest.json").map(Vec::as_slice).unwrap_or(b"");
    let mut manifest = json::unmarshal_value(raw).unwrap_or_else(|e| panic!("{e}"));
    if let Value::Object(m) = &mut manifest {
        for varying in ["created_at", "elysiandb_version"] {
            m.remove(varying);
        }
    }

    manifest
}

#[test]
fn an_archive_holds_the_same_files_as_those_of_earlier_versions() {
    let _engine = Engine;
    let fixture = std::fs::read(testdata(ARCHIVES_DIR).join("v0.2.4.tar.gz"))
        .unwrap_or_else(|e| panic!("{e}"));
    let (want_names, want_files) = read_archive(&fixture);

    let folder = copy_folder(&testdata(LEGACY_FIXTURES_DIR).join("full"));
    let _server = start_server_on_folder(folder.path(), true);

    let mut out = Vec::new();
    if let Err(e) = backup::create(&mut out, Time::now()) {
        panic!("{e}");
    }
    let (got_names, got_files) = read_archive(&out);

    if got_names != want_names {
        panic!("the files or their order changed:\n{got_names:?}\n{want_names:?}");
    }

    let mut errors = Vec::new();
    for name in &want_names {
        if name == "manifest.json" {
            continue;
        }
        if got_files.get(name) != want_files.get(name) {
            errors.push(format!(
                "{name} differs:\n{}\n{}",
                truncate(&String::from_utf8_lossy(
                    got_files.get(name).map(Vec::as_slice).unwrap_or(b"")
                )),
                truncate(&String::from_utf8_lossy(
                    want_files.get(name).map(Vec::as_slice).unwrap_or(b"")
                ))
            ));
        }
    }

    let got_manifest = manifest_without_varying_fields(&got_files);
    let want_manifest = manifest_without_varying_fields(&want_files);
    if got_manifest != want_manifest {
        panic!("the manifest differs:\n{got_manifest:?}\n{want_manifest:?}");
    }

    if !errors.is_empty() {
        panic!("{}", errors.join("\n"));
    }
}

#[test]
fn an_archive_of_this_version_is_restored_by_version024() {
    let binary = std::env::var("ELYSIANDB_V024_BINARY").unwrap_or_default();
    if binary.is_empty() {
        skip("needs a binary of 0.2.4 in ELYSIANDB_V024_BINARY");
        return;
    }

    let _engine = Engine;
    let want = golden_full();
    let source = copy_folder(&testdata(LEGACY_FIXTURES_DIR).join("full"));
    let server = start_server_on_folder(source.path(), true);

    let work = TempDir::new();
    let archive = work.path().join("archive.tar.gz");
    let f = std::fs::File::create(&archive).unwrap_or_else(|e| panic!("{e}"));
    if let Err(e) = backup::create(f, Time::now()) {
        panic!("{e}");
    }
    shutdown_engine();
    drop(server);

    let folder = TempDir::new();
    let config = work.path().join("config.yaml");
    let yaml = format!(
        "store:\n  folder: {}\n  shards: 8\n  flushIntervalSeconds: 0\n  crashRecovery: {{ enabled: true, maxLogMB: 100 }}\nengine:\n  name: internal\nserver:\n  http: {{ enabled: false, host: 127.0.0.1, port: 1 }}\n  tcp: {{ enabled: false, host: 127.0.0.1, port: 2 }}\nsecurity:\n  authentication: {{ enabled: true, mode: user }}\napi:\n  schema: {{ enabled: true, strict: true }}\n  hooks: {{ enabled: true }}\nlog: {{ level: error, format: text }}\n",
        folder.path().display()
    );
    std::fs::write(&config, yaml).unwrap_or_else(|e| panic!("{e}"));

    let out = Command::new(&binary)
        .arg("--config")
        .arg(&config)
        .arg("restore")
        .arg(&archive)
        .arg("--force")
        .output()
        .unwrap_or_else(|e| panic!("0.2.4 refuses the archive: {e}"));
    if !out.status.success() {
        panic!(
            "0.2.4 refuses the archive: {}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let mut errors = Vec::new();
    let mut s = start_server_on_folder(folder.path(), true);
    compare_with_golden(
        &mut errors,
        "folder written by 0.2.4 from an archive of this version",
        &mut s,
        &want,
    );

    if !errors.is_empty() {
        panic!("{}", errors.join("\n"));
    }
}
