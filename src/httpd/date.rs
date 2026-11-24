use std::time::{SystemTime, UNIX_EPOCH};

use super::bytesconv::append_http_date;
use crate::gocompat::time::Time;
use crate::sync::RwLock;

static CACHE: RwLock<(i64, Vec<u8>)> = RwLock::new((i64::MIN, Vec::new()));

fn unix_now() -> (i64, i64) {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => (
            i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            i64::from(d.subsec_nanos()),
        ),
        Err(_) => (0, 0),
    }
}

pub fn server_date() -> Vec<u8> {
    let (sec, nsec) = unix_now();
    {
        let c = CACHE.read();
        if c.0 == sec {
            return c.1.clone();
        }
    }
    let mut b = Vec::with_capacity(29);
    append_http_date(&mut b, &Time::from_unix(sec, nsec));
    let mut c = CACHE.write();
    *c = (sec, b.clone());
    b
}
