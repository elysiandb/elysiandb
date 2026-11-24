#[path = "../tempdir.rs"]
mod tempdir;

mod loader_test;
mod security_test;

use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}
