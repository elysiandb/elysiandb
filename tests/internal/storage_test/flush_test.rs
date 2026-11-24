use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use elysiandb::configuration::{CrashRecoveryConfig, StoreConfig};
use elysiandb::log::{self, AttrValue, Level, Options, Query};
use elysiandb::metrics;
use elysiandb::storage;

use super::tempdir::TempDir;
use super::{serial, set_store};

fn chmod(dir: &Path, mode: u32) -> std::io::Result<()> {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
}

struct Cleanup<'a> {
    dir: &'a Path,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = chmod(self.dir, 0o700);
        let _ = storage::close();
        log::set_output(None, false);
        let _ = log::configure(&Options::default());
        metrics::reset();
    }
}

fn error_lines() -> Vec<Arc<log::Entry>> {
    log::logs(&Query {
        min_level: Level::ERROR,
        ..Query::default()
    })
    .entries
}

#[test]
fn test_failed_checkpoint_is_reported_and_retried() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }

    let _serial = serial();
    let dir = TempDir::new();
    set_store(StoreConfig {
        folder: dir.path().to_string_lossy().into_owned(),
        shards: 4,
        crash_recovery: CrashRecoveryConfig {
            enabled: true,
            max_log_mb: 100,
        },
        ..StoreConfig::default()
    });

    if let Err(err) = log::configure(&Options {
        buffer_lines: 100,
        ..Options::default()
    }) {
        panic!("{err}");
    }
    log::set_output(Some(Box::new(std::io::sink())), false);
    metrics::reset();
    let _cleanup = Cleanup { dir: dir.path() };

    storage::load_db();
    if let Err(err) = storage::put_key_value(b"k", b"v") {
        panic!("{err}");
    }

    if let Err(err) = chmod(dir.path(), 0o500) {
        panic!("{err}");
    }

    if storage::checkpoint().is_ok() {
        panic!("a checkpoint in a read-only folder must fail");
    }

    let failures = error_lines();
    if failures.len() != 1
        || failures[0].message != "flush failed"
        || failures[0].component != b"storage"
        || failures[0].attrs.get("target") != Some(&AttrValue::Str(b"store".to_vec()))
    {
        panic!("expected one flush failed line, got {failures:?}");
    }
    let p = metrics::persistence();
    if p.last_flush_ok || p.flush_failures != 1 {
        panic!("the failure must be visible in the metrics: {p:?}");
    }

    let _ = storage::checkpoint();
    let got = error_lines();
    if got.len() != 1 {
        panic!("repeated failures are throttled, got {} lines", got.len());
    }

    if let Err(err) = chmod(dir.path(), 0o700) {
        panic!("{err}");
    }

    if let Err(err) = storage::checkpoint() {
        panic!("{err}");
    }

    let recovered = log::logs(&Query {
        text: b"flush working again".to_vec(),
        ..Query::default()
    })
    .entries;
    if recovered.len() != 1 || recovered[0].attrs.get("failures") != Some(&AttrValue::Int(2)) {
        panic!("expected a recovery line with 2 failures, got {recovered:?}");
    }

    let p = metrics::persistence();
    if !p.last_flush_ok || p.last_success_at.is_none() {
        panic!("persistence should be ok again: {p:?}");
    }

    if let Err(err) = storage::close() {
        panic!("{err}");
    }

    storage::load_db();
    match storage::get_by_key(b"k") {
        Ok(v) if v == b"v" => {}
        other => panic!("the key survives a restart: {other:?}"),
    }
}
