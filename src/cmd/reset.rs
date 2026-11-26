use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::globals;
use crate::gocompat::strings::trim_space;
use crate::storage;

use super::cmd::{os_args, printf, read_line};

pub fn reset_all() {
    printf("Are you sure you want to reset the database ? This action cannot be undone.\n");
    printf("Make sure ElysianDB is down before proceeding.\n");
    printf("(yes/no)\n");

    let response = read_line();
    if trim_space(&response) != b"yes" {
        printf("Reset aborted.\n");
        return;
    }

    let force = os_args().iter().any(|arg| arg == b"--force");
    if !force {
        printf("Reset requires --force flag to proceed.\n");
        std::process::exit(1);
    }

    let Some(cfg) = globals::config() else {
        panic!("the configuration is not set");
    };
    let dir = cfg.store.folder.clone();

    if let Err(err) = storage::close() {
        printf(format!("Cannot close the data folder cleanly: {err}\n"));
    }

    let _ = remove_all(dir.as_bytes());
}

pub(crate) fn remove_all(path: &[u8]) -> io::Result<()> {
    if path.is_empty() {
        return Ok(());
    }

    if path == b"." || path.ends_with(b"/.") {
        return Err(io::Error::from_raw_os_error(libc::EINVAL));
    }

    let p = Path::new(OsStr::from_bytes(path));
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
    }
}
