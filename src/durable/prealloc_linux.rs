use super::fs::OsFile;

const FALLOC_KEEP_SIZE: libc::c_int = 1;

pub(super) fn preallocate(f: &OsFile, size: i64) {
    let _ =
        f.with_fd(|fd| unsafe { libc::fallocate(fd, FALLOC_KEEP_SIZE, 0, size as libc::off_t) });
}
