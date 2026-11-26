use std::error::Error as StdError;
use std::ffi::OsStr;
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::backup::{self, Report};
use crate::durable::FsError;
use crate::globals::{GOLD, RED, RESET};
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::quote_text;
use crate::gocompat::strings::trim_space;

use super::cmd::{command_args, exit, printf, read_line_from, with_stdin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestoreAborted;

impl fmt::Display for RestoreAborted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("restore aborted")
    }
}

impl StdError for RestoreAborted {}

pub const ERR_RESTORE_ABORTED: RestoreAborted = RestoreAborted;

const RESTORE_USAGE: &str = "usage: elysiandb restore <file> [--force]";

pub fn restore() {
    let args = command_args();
    let result = with_stdin(|input| run_restore(&args, input));

    let report = match result {
        Ok(report) => report,
        Err(err) if errors::is(err.as_ref(), &ERR_RESTORE_ABORTED) => {
            printf("Restore aborted.\n");
            return;
        }
        Err(err) => {
            printf(format!("{RED}Restore failed: {err}{RESET}\n"));
            exit(1);
            return;
        }
    };

    printf(format!("{GOLD}Restore completed.{RESET}\n"));
    for (entity, count) in &report.entities {
        let mut line = b"  ".to_vec();
        line.extend_from_slice(entity);
        line.extend_from_slice(format!(": {count} documents\n").as_bytes());
        printf(line);
    }

    printf(format!("  schemas: {}\n", report.schemas));
    printf(format!(
        "  KV keys: {} ({} expired and skipped)\n",
        report.kv_keys, report.expired_kv_keys
    ));
}

pub fn run_restore<S: AsRef<[u8]>>(args: &[S], input: &mut dyn Read) -> Result<Report, BoxError> {
    let mut file: &[u8] = b"";
    let mut force = false;
    for arg in args {
        let arg = arg.as_ref();
        if arg == b"--force" || arg == b"-force" {
            force = true;
        } else if arg.starts_with(b"-") {
            return Err(errors::new(format!(
                "unknown argument {}, usage: elysiandb restore <file> [--force]",
                quote_text(arg)
            )));
        } else if file.is_empty() {
            file = arg;
        } else {
            return Err(errors::new(RESTORE_USAGE));
        }
    }

    if file.is_empty() {
        return Err(errors::new(RESTORE_USAGE));
    }

    let path = Path::new(OsStr::from_bytes(file));
    let mut f = File::open(path).map_err(|e| FsError::new("open", path, e))?;

    let manifest = backup::verify(&mut f)?;

    if !force && !confirm_restore(input) {
        return Err(Box::new(ERR_RESTORE_ABORTED));
    }

    f.seek(SeekFrom::Start(0))
        .map_err(|e| FsError::new("seek", path, e))?;

    backup::apply(f, &manifest)
}

fn confirm_restore(input: &mut dyn Read) -> bool {
    printf(
        "The restore replaces all the data, users and KV keys with the content of the archive. This action cannot be undone.\n",
    );
    printf("Make sure ElysianDB is down before proceeding.\n");
    printf("Type yes to continue:\n");

    let response = read_line_from(input);

    trim_space(&response) == b"yes"
}
