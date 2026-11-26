use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::durable::{FsError, filepath};
use crate::globals::{self, GOLD, GRAY, RED, RESET};
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::quote_text;
use crate::legacy;
use crate::security::{KEY_FILENAME, SESSIONS_FILENAME};
use crate::storage;

use super::cmd::{command_args, exit, printf};

pub fn close_storage() {
    if let Err(err) = storage::close() {
        printf(format!(
            "{RED}Cannot close the data folder cleanly: {err}{RESET}\n"
        ));
    }
}

pub fn export_legacy() {
    let folder = match run_export_legacy(&command_args()) {
        Ok(folder) => folder,
        Err(err) => {
            printf(format!("{RED}Export failed: {err}{RESET}\n"));
            close_storage();
            exit(1);
            return;
        }
    };

    let mut out = GOLD.as_bytes().to_vec();
    out.extend_from_slice(b"A 0.2.4 data folder was written to ");
    out.extend_from_slice(folder.as_os_str().as_bytes());
    out.extend_from_slice(RESET.as_bytes());
    out.push(b'\n');
    printf(out);
    printf(format!(
        "{GRAY}Start ElysianDB 0.2.4 with store.folder set to it to go back.{RESET}\n"
    ));
}

pub fn run_export_legacy<S: AsRef<[u8]>>(args: &[S]) -> Result<PathBuf, BoxError> {
    let mut output: &[u8] = b"";
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_ref();
        if arg == b"--output" || arg == b"-output" || arg == b"-o" {
            if i + 1 >= args.len() {
                return Err(errors::new("--output needs a folder"));
            }

            i += 1;
            output = args[i].as_ref();
        } else if let Some(rest) = arg.strip_prefix(b"--output=") {
            output = rest;
        } else {
            return Err(errors::new(format!(
                "unknown argument {}, usage: elysiandb export-legacy --output <folder>",
                quote_text(arg)
            )));
        }
        i += 1;
    }

    if output.is_empty() {
        return Err(errors::new(
            "usage: elysiandb export-legacy --output <folder>",
        ));
    }

    let folder = PathBuf::from(OsStr::from_bytes(output));
    if let Ok(entries) = fs::read_dir(&folder)
        && let Ok(entries) = entries.collect::<Result<Vec<_>, _>>()
        && !entries.is_empty()
    {
        return Err(errors::new(format!(
            "{} is not empty",
            String::from_utf8_lossy(output)
        )));
    }

    let Some(e) = storage::engine() else {
        return Err(errors::new("the data folder is not open"));
    };

    e.export_legacy(&folder)?;

    let Some(cfg) = globals::config() else {
        panic!("the configuration is not set");
    };
    let store = Path::new(&cfg.store.folder);
    for name in [KEY_FILENAME, SESSIONS_FILENAME] {
        let src = filepath::join(store, name);
        let mut f = match fs::File::open(&src) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(Box::new(FsError::new("open", &src, e))),
        };
        let mut data = Vec::new();
        f.read_to_end(&mut data)
            .map_err(|e| FsError::new("read", &src, e))?;

        write_file(&filepath::join(&folder, name), &data)?;
    }

    if legacy::files_in(&folder).is_empty() {
        return Err(errors::new("nothing was written"));
    }

    Ok(folder)
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), FsError> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| FsError::new("open", path, e))?;
    f.write_all(data)
        .map_err(|e| FsError::new("write", path, e))
}
