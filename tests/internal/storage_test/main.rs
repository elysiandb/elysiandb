#[path = "../tempdir.rs"]
mod tempdir;

mod file_mode_test;
mod flush_test;
mod reset_test;

use std::sync::Arc;

use elysiandb::configuration::{Config, StoreConfig};
use elysiandb::globals;
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}

pub struct CloseStorage;

impl Drop for CloseStorage {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

pub fn set_store(store: StoreConfig) {
    globals::set_config(Some(Arc::new(Config {
        store,
        ..Config::default()
    })));
}
