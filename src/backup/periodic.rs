use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::durable::{self, Fs, FsError, filepath};
use crate::globals;
use crate::gocompat::time::Time;
use crate::log::Value;
use crate::sync::{Stop, Ticker, spawn};

use super::archive::{FILE_NAME_EXTENSION, FILE_NAME_PREFIX};
use super::create::{Options, write_file_with};
use super::status::BACKUP_LOG;

const DEFAULT_KEPT_BACKUPS: i64 = 7;
const MINUTE_NANOS: i64 = 60_000_000_000;

pub fn start_periodic() -> Box<dyn FnOnce() + Send> {
    let Some(cfg) = globals::config() else {
        panic!("the configuration is not set");
    };
    let backup = cfg.store.backup.clone();
    if backup.every_minutes <= 0 || backup.folder.is_empty() {
        return Box::new(|| {});
    }

    let nanos = backup.every_minutes.wrapping_mul(MINUTE_NANOS);
    let period = if nanos > 0 {
        Duration::from_nanos(nanos as u64)
    } else {
        Duration::ZERO
    };

    let stop = Arc::new(Stop::new());
    let signal = stop.clone();
    spawn("backup-periodic", move || {
        let mut ticker = Ticker::new(period);
        while ticker.wait(&signal).is_some() {
            run_periodic(&backup.folder, backup.keep);
        }
    });

    Box::new(move || stop.stop())
}

pub fn run_periodic(folder: &str, keep: i64) {
    let path = Path::new(folder);
    if let Err(err) = durable::OS.mkdir_all(path, 0o700) {
        let text = err.to_string();
        BACKUP_LOG.error(
            "cannot create the backup folder",
            &[
                ("folder", Value::Str(folder.as_bytes())),
                ("err", Value::Str(text.as_bytes())),
            ],
        );
        return;
    }

    durable::run_gently(|| {
        let opts = Options {
            pace: true,
            ..Options::default()
        };
        if let Err(err) = write_file_with(path, Time::now().utc(), opts) {
            let text = err.to_string();
            BACKUP_LOG.error(
                "periodic backup failed",
                &[
                    ("folder", Value::Str(folder.as_bytes())),
                    ("err", Value::Str(text.as_bytes())),
                ],
            );
            return;
        }

        prune_backups(path, keep);
    });
}

fn prune_backups(folder: &Path, keep: i64) {
    let keep = if keep <= 0 {
        DEFAULT_KEPT_BACKUPS
    } else {
        keep
    };

    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    let mut names: Vec<Vec<u8>> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return;
        };

        let name = entry.file_name().as_bytes().to_vec();
        if name.starts_with(FILE_NAME_PREFIX.as_bytes())
            && name.ends_with(FILE_NAME_EXTENSION.as_bytes())
        {
            names.push(name);
        }
    }

    names.sort();
    let excess = names.len() as i64 - keep;
    for name in names.iter().take(excess.max(0) as usize) {
        let path = filepath::join(folder, std::ffi::OsStr::from_bytes(name));
        if let Err(e) = fs::remove_file(&path) {
            let text = FsError::new("remove", &path, e).to_string();
            BACKUP_LOG.warn(
                "cannot remove an old backup",
                &[
                    ("file", Value::Str(name)),
                    ("err", Value::Str(text.as_bytes())),
                ],
            );
        }
    }
}
