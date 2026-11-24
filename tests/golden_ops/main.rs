#[path = "../common/mod.rs"]
mod common;
#[path = "../internal/tempdir.rs"]
mod tempdir;

mod archives;
mod config;
mod fmt;
mod logs;
mod metrics;
mod tar_archives;
mod tcp;
mod yaml;

use elysiandb::log::{self, Options};
use elysiandb::sync::{Mutex, MutexGuard};
use std::io::{self, Write};
use std::sync::Arc;

static LOG_STATE: Mutex<()> = Mutex::new(());

fn log_state() -> MutexGuard<'static, ()> {
    LOG_STATE.lock()
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(p);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    fn take(&self) -> String {
        let bytes = std::mem::take(&mut *self.0.lock());
        String::from_utf8(bytes).expect("log output is UTF-8")
    }
}

struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        log::set_output(None, false);
        let _ = log::configure(&Options::default());
    }
}
