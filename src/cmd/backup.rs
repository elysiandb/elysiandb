use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::backup as archive;
use crate::durable::FsError;
use crate::globals::{GOLD, GRAY, RED, RESET};
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::quote_text;
use crate::gocompat::time::Time;

use super::backup_remote::{parse_remote_args, runs_without_storage};
use super::cmd::{BACKUP_COMMAND, command_args, exit, printf};

pub fn backup() {
    let path = match run_backup(&command_args()) {
        Ok(path) => path,
        Err(err) => {
            printf(format!("{RED}Backup failed: {err}{RESET}\n"));
            exit(1);
            return;
        }
    };

    let mut out = GOLD.as_bytes().to_vec();
    out.extend_from_slice(b"Backup written to ");
    out.extend_from_slice(path.as_os_str().as_bytes());
    out.extend_from_slice(RESET.as_bytes());
    out.push(b'\n');
    printf(out);
    printf(format!(
        "{GRAY}The archive holds the users key and the password hashes: store it as safely as the data folder.{RESET}\n"
    ));
}

pub fn run_backup<S: AsRef<[u8]>>(args: &[S]) -> Result<PathBuf, BoxError> {
    let mut all: Vec<&[u8]> = vec![BACKUP_COMMAND.as_bytes()];
    all.extend(args.iter().map(AsRef::as_ref));
    if runs_without_storage(&all) {
        let remote = parse_remote_args(args)?;
        check_folder(&remote.output)?;
        return remote.run();
    }

    let mut output: &[u8] = b".";
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
                "unknown argument {}, usage: elysiandb backup [--output <folder>] [--url <address>]",
                quote_text(arg)
            )));
        }
        i += 1;
    }

    check_folder(output)?;

    let (path, _) = archive::write_file(Path::new(OsStr::from_bytes(output)), Time::now().utc())?;

    Ok(path)
}

pub(crate) fn check_folder(folder: &[u8]) -> Result<(), BoxError> {
    let path = Path::new(OsStr::from_bytes(folder));
    let meta = fs::metadata(path).map_err(|e| FsError::new("stat", path, e))?;
    if !meta.is_dir() {
        return Err(errors::new(format!(
            "{} is not a folder",
            String::from_utf8_lossy(folder)
        )));
    }

    Ok(())
}
