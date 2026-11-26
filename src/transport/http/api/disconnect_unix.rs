use std::io;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::guard::AbandonedProbe;
use crate::httpd::ConnHandle;

const DISCONNECT_PROBE_INTERVAL_NANOS: i64 = 10_000_000;

fn unix_nano() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_nanos()).unwrap_or(i64::MAX),
    }
}

fn peer_gone(conn: &ConnHandle) -> bool {
    if conn.is_closed() {
        return true;
    }

    let mut b = [0u8; 1];
    let n = unsafe {
        libc::recv(
            conn.raw_fd(),
            b.as_mut_ptr().cast::<libc::c_void>(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    if n == 0 {
        return true;
    }
    if n > 0 {
        return false;
    }

    matches!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECONNRESET) | Some(libc::ENOTCONN) | Some(libc::EPIPE)
    )
}

pub fn disconnect_probe(conn: Option<&ConnHandle>) -> Option<AbandonedProbe> {
    let conn = conn?.clone();
    let last = AtomicI64::new(0);
    let gone = AtomicBool::new(false);

    Some(Box::new(move || {
        if gone.load(Ordering::SeqCst) {
            return true;
        }

        let now = unix_nano();
        let prev = last.load(Ordering::SeqCst);
        if now.wrapping_sub(prev) < DISCONNECT_PROBE_INTERVAL_NANOS
            || last
                .compare_exchange(prev, now, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return false;
        }

        let closed = peer_gone(&conn);
        if closed {
            gone.store(true, Ordering::SeqCst);
        }

        closed
    }))
}
