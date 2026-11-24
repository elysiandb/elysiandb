mod cache_test;
mod store_test;

use std::collections::BTreeMap;

use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}

pub type Filters = BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Vec<u8>>>;

pub fn filters(list: &[(&str, &[(&str, &str)])]) -> Filters {
    list.iter()
        .map(|(field, ops)| {
            let ops = ops
                .iter()
                .map(|(op, value)| (op.as_bytes().to_vec(), value.as_bytes().to_vec()))
                .collect();
            (field.as_bytes().to_vec(), ops)
        })
        .collect()
}
