use std::io::{self, Cursor, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use elysiandb::api;
use elysiandb::archive::gzip;
use elysiandb::archive::tar;
use elysiandb::backup::{self, ArchiveError, IncompleteRestoreError, Manifest, Options};
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map};
use elysiandb::gocompat::time::Time;
use elysiandb::log::{self, Options as LogOptions};
use elysiandb::storage;

use super::archives::archive;
use super::backup_archives::open_store;
use super::tempdir::TempDir;
use super::{Capture, Restore, log_state};

fn capture_logs() -> Capture {
    log::configure(&LogOptions {
        level: "info".to_string(),
        format: "text".to_string(),
        ..LogOptions::default()
    })
    .expect("configure");
    let capture = Capture::default();
    log::set_output(Some(Box::new(capture.clone())), false);
    capture
}

fn object(text: &str) -> Map {
    let mut m = Map::new();
    json::unmarshal(text.as_bytes(), &mut m).unwrap_or_else(|e| panic!("{text}: {e}"));
    m
}

struct Failing;

impl Write for Failing {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("disk full"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Panicking(bool);

impl Write for Panicking {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        if self.0 {
            std::panic::panic_any(42);
        }
        panic!("boom");
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn line_with<'a>(lines: &'a str, msg: &str) -> &'a str {
    lines
        .lines()
        .find(|l| l.contains(&format!("msg=\"{msg}\"")))
        .unwrap_or_else(|| panic!("no {msg} line in {lines}"))
}

#[test]
fn each_backup_end_updates_the_status_and_logs_once() {
    let _state = log_state();
    let _restore = Restore;
    let capture = capture_logs();
    let _store = open_store();

    api::write_entity(b"books", &mut object(r#"{"id":"b1"}"#));
    api::write_entity(b"books", &mut object(r#"{"id":"b2"}"#));
    storage::put_key_value(b"k", b"v").expect("put");

    let before = backup::current_status();
    let mut buf = Vec::new();
    let manifest = backup::create(&mut buf, Time::now()).expect("backup");
    let status = backup::current_status();
    assert_eq!(status.completed, before.completed + 1);
    assert_eq!(status.failed, before.failed);
    assert_eq!(status.running, 0);
    assert!(status.last_ok);
    assert!(status.last_at.is_some());
    assert_eq!(status.last_error, "");
    assert_eq!(status.bytes, buf.len() as i64);
    let documents: i64 = manifest.entities.iter().map(|e| e.documents).sum();
    assert_eq!(status.documents, documents);
    assert_eq!(documents, 2);

    let logs = capture.take();
    let line = line_with(&logs, "backup written");
    assert!(line.contains(" level=INFO "), "{line}");
    assert!(
        line.contains(&format!(
            "component=backup documents=2 bytes={} duration_ms=",
            buf.len()
        )),
        "{line}"
    );

    let mut src = backup::prepare(Time::now()).expect("prepare");
    let err = src
        .write_archive(Failing, Options::default())
        .expect_err("a failing destination");
    assert_eq!(err.to_string(), "disk full");
    src.close();
    let status = backup::current_status();
    assert_eq!(status.failed, before.failed + 1);
    assert!(!status.last_ok);
    assert_eq!(status.last_error, "disk full");
    assert_eq!(status.bytes, 0);
    let logs = capture.take();
    let line = line_with(&logs, "backup failed");
    assert!(line.contains(" level=ERROR "), "{line}");
    assert!(
        line.contains("component=backup err=\"disk full\" duration_ms="),
        "{line}"
    );

    for (unknown, want) in [
        (false, "the backup stopped: boom"),
        (true, "the backup stopped: unknown panic"),
    ] {
        let mut src = backup::prepare(Time::now()).expect("prepare");
        let err = src
            .write_archive(Panicking(unknown), Options::default())
            .expect_err("a panicking destination");
        assert_eq!(err.to_string(), want);
        assert_eq!(backup::current_status().last_error, want);
        assert_eq!(backup::current_status().running, 0);
    }
    assert_eq!(backup::current_status().failed, before.failed + 3);
}

fn backups_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .expect("folder")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn periodic_backups_log_their_failures_and_keep_the_newest_archives() {
    let _state = log_state();
    let _restore = Restore;
    let capture = capture_logs();
    let store = open_store();

    let file = store.path().join("not-a-folder");
    std::fs::write(&file, b"x").expect("file");
    let file_text = file.to_string_lossy().into_owned();
    backup::run_periodic(&file_text, 3);
    let logs = capture.take();
    let line = line_with(&logs, "cannot create the backup folder");
    assert!(line.contains(" level=ERROR "), "{line}");
    assert!(
        line.contains(&format!(
            "component=backup folder={file_text} err=\"mkdir {file_text}: Not a directory (os error 20)\""
        )),
        "{line}"
    );

    let folder = store.path().join("backups");
    std::fs::create_dir(&folder).expect("folder");
    let old = [
        "elysiandb-backup-20200101T000000Z.tar.gz",
        "elysiandb-backup-20200102T000000Z.tar.gz",
        "elysiandb-backup-20200103T000000Z.tar.gz",
        "elysiandb-backup-20200104T000000Z.tar.gz",
        "elysiandb-backup-20200105T000000Z.tar.gz",
    ];
    for name in old
        .iter()
        .chain(["notes.txt", "elysiandb-backup-x.tar"].iter())
    {
        std::fs::write(folder.join(name), b"old").expect("old file");
    }

    let folder_text = folder.to_string_lossy().into_owned();
    backup::run_periodic(&folder_text, 3);
    let names = backups_in(&folder);
    assert_eq!(names.len(), 5, "{names:?}");
    assert_eq!(&names[..2], &old[3..]);
    assert!(names[2].starts_with("elysiandb-backup-2"), "{names:?}");
    assert_eq!(&names[3..], ["elysiandb-backup-x.tar", "notes.txt"]);
    let logs = capture.take();
    assert!(logs.contains("msg=\"backup written\""), "{logs}");

    for name in old {
        std::fs::write(folder.join(name), b"old").expect("old file");
    }
    std::thread::sleep(std::time::Duration::from_millis(1100));
    backup::run_periodic(&folder_text, 0);
    let kept = backups_in(&folder)
        .iter()
        .filter(|n| n.ends_with(".tar.gz"))
        .count();
    assert_eq!(kept, 7);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o500)).expect("chmod");
    backup::run_periodic(&folder_text, 3);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let logs = capture.take();
    let line = line_with(&logs, "periodic backup failed");
    assert!(line.contains(" level=ERROR "), "{line}");
    assert!(
        line.contains(&format!(
            "component=backup folder={folder_text} err=\"open {folder_text}/.elysiandb-backup-"
        )),
        "{line}"
    );
    assert!(
        line.ends_with(": Permission denied (os error 13)\""),
        "{line}"
    );
}

fn set_backup_config(every_minutes: i64, folder: &str) {
    let mut cfg = (*globals::config().expect("config")).clone();
    cfg.store.backup.every_minutes = every_minutes;
    cfg.store.backup.folder = folder.to_string();
    globals::set_config(Some(Arc::new(cfg)));
}

#[test]
fn periodic_backups_start_only_with_a_period_and_a_folder() {
    let store = open_store();
    let folder = store.path().join("periodic");
    let folder_text = folder.to_string_lossy().into_owned();

    for (every, target) in [
        (0, folder_text.as_str()),
        (-1, folder_text.as_str()),
        (5, ""),
    ] {
        set_backup_config(every, target);
        let stop = backup::start_periodic();
        stop();
    }

    set_backup_config(1, &folder_text);
    let stop = backup::start_periodic();
    std::thread::sleep(std::time::Duration::from_millis(50));
    stop();
    assert!(!folder.exists());
}

fn manifest_inside(data: &[u8]) -> Manifest {
    let gz = gzip::new_reader(data).expect("gzip");
    let mut tr = tar::Reader::new(gz);
    tr.next().expect("tar").expect("manifest entry");
    let mut raw = Vec::new();
    tr.read_to_end(&mut raw).expect("manifest");
    let mut manifest = Manifest::default();
    json::unmarshal(&raw, &mut manifest).expect("manifest json");
    manifest
}

#[test]
fn an_interrupted_restore_is_reported_until_a_restore_ends() {
    let _store = open_store();
    assert!(!backup::restore_was_interrupted());

    let data = archive("invalid/document-garbage.tar.gz");
    let err = backup::apply(Cursor::new(data.clone()), &manifest_inside(&data))
        .expect_err("garbage documents");
    let incomplete = err
        .downcast_ref::<IncompleteRestoreError>()
        .expect("an incomplete restore");
    assert!(incomplete.err.downcast_ref::<ArchiveError>().is_some());
    assert_eq!(
        err.to_string(),
        "the restore failed after the data was replaced, run it again: invalid backup archive: entities/books.jsonl, document 1: invalid JSON at byte 1: unexpected character 'x'"
    );
    assert!(backup::restore_was_interrupted());

    let marker = Path::new(&globals::config().expect("config").store.folder).join("RESTORING");
    let stamp = String::from_utf8(std::fs::read(&marker).expect("marker")).expect("utf8");
    assert!(stamp.ends_with("Z\n") && stamp.len() == 21, "{stamp:?}");

    backup::restore(Cursor::new(archive("valid/empty-entities.tar.gz"))).expect("restore");
    assert!(!backup::restore_was_interrupted());
}

#[test]
fn abandoned_uploads_are_removed_like_go_does() {
    let _ = storage::close();
    let dir = TempDir::new();
    for (name, removed) in [("data", true), ("da[ta]", false), ("d*", true)] {
        let folder = dir.path().join(name);
        let mut cfg = Config::default();
        cfg.store.folder = folder.to_string_lossy().into_owned();
        globals::set_config(Some(Arc::new(cfg)));

        let (mut file, path) = backup::new_upload().expect("upload");
        file.write_all(b"partial").expect("write");
        drop(file);
        let base = path.file_name().unwrap().to_string_lossy().into_owned();
        let digits = base.strip_prefix(".restore-upload-").expect("prefix");
        assert!(
            !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()),
            "{base}"
        );
        assert_eq!(path.parent(), Some(folder.as_path()));
        let mode = std::fs::metadata(&path)
            .expect("upload")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);

        backup::remove_abandoned_uploads();
        assert_eq!(!path.exists(), removed, "{name}");
    }
}

struct TmpDirVar(Option<std::ffi::OsString>);

impl Drop for TmpDirVar {
    fn drop(&mut self) {
        match self.0.take() {
            Some(v) => unsafe { std::env::set_var("TMPDIR", v) },
            None => unsafe { std::env::remove_var("TMPDIR") },
        }
    }
}

fn entry_names(data: &[u8]) -> Vec<String> {
    let mut tr = tar::Reader::new(gzip::new_reader(data).expect("gzip"));
    let mut names = Vec::new();
    while let Some(h) = tr.next().expect("tar") {
        names.push(String::from_utf8_lossy(&h.name).into_owned());
    }
    names
}

#[test]
fn a_backup_without_an_open_store_spools_its_entries_in_a_private_folder() {
    let _ = storage::close();
    let dir = TempDir::new();
    let spool = dir.path().join("tmp");
    std::fs::create_dir(&spool).expect("tmp");
    let _tmpdir = TmpDirVar(std::env::var_os("TMPDIR"));
    unsafe { std::env::set_var("TMPDIR", &spool) };

    let mut cfg = Config::default();
    cfg.store.folder = dir.path().join("data").to_string_lossy().into_owned();
    globals::set_config(Some(Arc::new(cfg)));
    std::fs::create_dir(dir.path().join("data")).expect("data");

    let mut src = backup::prepare(Time::now()).expect("prepare");
    let folders: Vec<_> = std::fs::read_dir(&spool)
        .expect("spool")
        .flatten()
        .collect();
    assert_eq!(folders.len(), 1);
    let name = folders[0].file_name().to_string_lossy().into_owned();
    let digits = name.strip_prefix("elysiandb-backup-").expect("prefix");
    assert!(digits.bytes().all(|c| c.is_ascii_digit()), "{name}");
    let mode = folders[0].metadata().expect("meta").permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    let files: Vec<String> = std::fs::read_dir(folders[0].path())
        .expect("spooled")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(files.iter().all(|f| f.starts_with("entry-")), "{files:?}");

    let mut buf = Vec::new();
    src.write_archive(&mut buf, Options::default())
        .expect("backup");
    src.close();
    assert_eq!(std::fs::read_dir(&spool).expect("spool").count(), 0);

    let manifest = backup::verify(buf.as_slice()).expect("verify");
    assert!(manifest.entities.is_empty());
    assert!(manifest.schemas.is_empty());
    assert_eq!(manifest.kv_keys, 0);
    assert_eq!(manifest.files.len(), 2);
    assert_eq!(
        entry_names(&buf),
        [backup::MANIFEST_PATH, backup::KEY_PATH, backup::KV_PATH]
    );
}
