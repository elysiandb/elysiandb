#[path = "../tempdir.rs"]
mod tempdir;

mod atomic_test;
mod transaction_test;

use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}
