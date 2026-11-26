#[path = "../internal/tempdir.rs"]
mod tempdir;

mod adminui;
mod boot;
mod cache;
mod hook;
mod http_hook;
mod schema;
mod storage;
mod tcp_routing;

use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}
