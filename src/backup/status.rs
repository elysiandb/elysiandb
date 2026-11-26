use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use crate::gocompat::errors::BoxError;
use crate::log::{self, Logger, Value};
use crate::metrics::Moment;
use crate::sync::Mutex;

use super::archive::Manifest;

pub(super) static BACKUP_LOG: Logger = log::for_component("backup");

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub running: i64,
    pub completed: i64,
    pub failed: i64,
    pub last_at: Option<Moment>,
    pub last_ok: bool,
    pub duration: Duration,
    pub bytes: i64,
    pub documents: i64,
    pub last_error: String,
}

static STATUS: Mutex<Status> = Mutex::new(Status {
    running: 0,
    completed: 0,
    failed: 0,
    last_at: None,
    last_ok: false,
    duration: Duration::ZERO,
    bytes: 0,
    documents: 0,
    last_error: String::new(),
});

static RUNNING: AtomicI64 = AtomicI64::new(0);

pub fn current_status() -> Status {
    let status = STATUS.lock();

    let mut out = status.clone();
    out.running = RUNNING.load(Ordering::SeqCst);

    out
}

pub(super) fn begin() -> impl FnOnce(&Manifest, i64, Option<&BoxError>) {
    RUNNING.fetch_add(1, Ordering::SeqCst);
    let start = Instant::now();

    move |m: &Manifest, bytes: i64, err: Option<&BoxError>| {
        RUNNING.fetch_add(-1, Ordering::SeqCst);

        let mut documents = 0i64;
        for e in &m.entities {
            documents += e.documents;
        }

        {
            let mut status = STATUS.lock();
            status.last_at = Some(Moment::now());
            status.last_ok = err.is_none();
            status.duration = start.elapsed();
            status.bytes = bytes;
            status.documents = documents;
            match err {
                None => {
                    status.completed += 1;
                    status.last_error = String::new();
                }
                Some(err) => {
                    status.failed += 1;
                    status.last_error = err.to_string();
                }
            }
        }

        if let Some(err) = err {
            let text = err.to_string();
            BACKUP_LOG.error(
                "backup failed",
                &[
                    ("err", Value::Str(text.as_bytes())),
                    ("duration_ms", Value::Float(log::since(start))),
                ],
            );
            return;
        }

        BACKUP_LOG.info(
            "backup written",
            &[
                ("documents", Value::Int(documents)),
                ("bytes", Value::Int(bytes)),
                ("duration_ms", Value::Float(log::since(start))),
            ],
        );
    }
}
