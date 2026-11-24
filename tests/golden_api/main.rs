#[path = "../common/mod.rs"]
mod common;
#[path = "../internal/tempdir.rs"]
mod tempdir;

mod cache_keys;
mod schema_corpus;

use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}
