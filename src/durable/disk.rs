use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use super::fs::{File, FsError};
use super::prealloc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoStats {
    pub read_bytes: i64,
    pub write_bytes: i64,
    pub syncs: i64,
    pub sync_nanos: i64,
}

pub(crate) static IO_READ_BYTES: AtomicI64 = AtomicI64::new(0);
pub(crate) static IO_WRITE_BYTES: AtomicI64 = AtomicI64::new(0);
static SYNCS: AtomicI64 = AtomicI64::new(0);
static SYNC_NANOS: AtomicI64 = AtomicI64::new(0);

pub fn stats() -> IoStats {
    IoStats {
        read_bytes: IO_READ_BYTES.load(Ordering::Relaxed),
        write_bytes: IO_WRITE_BYTES.load(Ordering::Relaxed),
        syncs: SYNCS.load(Ordering::Relaxed),
        sync_nanos: SYNC_NANOS.load(Ordering::Relaxed),
    }
}

pub(crate) fn note_sync(start: Instant) {
    SYNCS.fetch_add(1, Ordering::Relaxed);
    SYNC_NANOS.fetch_add(start.elapsed().as_nanos() as i64, Ordering::Relaxed);
}

#[allow(clippy::unnecessary_cast)]
pub fn free_bytes(dir: &Path) -> Result<i64, FsError> {
    let Ok(c) = CString::new(dir.as_os_str().as_bytes()) else {
        return Err(FsError::errno(libc::EINVAL));
    };
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return Err(FsError::bare(io::Error::last_os_error()));
    }
    Ok((st.f_bavail as i64).wrapping_mul(st.f_bsize as i64))
}

const PREALLOCATE_FREE_FACTOR: i64 = 8;

pub fn preallocate(f: &dyn File, dir: &Path, size: i64) {
    let Some(c) = f.os_file() else {
        return;
    };
    if size <= 0 {
        return;
    }
    match free_bytes(dir) {
        Ok(free) if free >= PREALLOCATE_FREE_FACTOR.wrapping_mul(size) => {}
        _ => return,
    }
    prealloc::preallocate(c, size);
}

pub fn trim_preallocated(f: &dyn File) {
    let Some(c) = f.os_file() else {
        return;
    };
    if let Ok(size) = c.size() {
        let _ = c.truncate(size);
    }
}
