use std::ffi::OsStr;
use std::fs;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::backup;
use crate::durable::{self, Fs, FsError, filepath};
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::{append_string, quote_text};
use crate::gocompat::strings::trim_space;
use crate::gocompat::time::Time;
use crate::httpd::parse_media_type;

use super::cmd::BACKUP_COMMAND;
use super::http_client::{Client, Request};

pub const REMOTE_PASSWORD_ENV: &str = "ELYSIANDB_PASSWORD";

const COPY_BUFFER: usize = 32 << 10;

pub(crate) struct RemoteBackup {
    url: Vec<u8>,
    user: Vec<u8>,
    password: Vec<u8>,
    token: Vec<u8>,
    pub(crate) output: Vec<u8>,
    verify: bool,
}

pub fn runs_without_storage<S: AsRef<[u8]>>(args: &[S]) -> bool {
    if args.is_empty() || args[0].as_ref() != BACKUP_COMMAND.as_bytes() {
        return false;
    }

    args[1..].iter().any(|arg| {
        let arg = arg.as_ref();
        arg == b"--url" || arg == b"-url" || arg.starts_with(b"--url=")
    })
}

fn cut(s: &[u8], sep: u8) -> (&[u8], &[u8], bool) {
    match s.iter().position(|&c| c == sep) {
        Some(i) => (&s[..i], &s[i + 1..], true),
        None => (s, &[], false),
    }
}

pub(crate) fn parse_remote_args<S: AsRef<[u8]>>(args: &[S]) -> Result<RemoteBackup, BoxError> {
    let mut r = RemoteBackup {
        url: Vec::new(),
        user: Vec::new(),
        password: Vec::new(),
        token: Vec::new(),
        output: b".".to_vec(),
        verify: true,
    };

    let value = |arg: &[u8], i: &mut usize| -> Result<Vec<u8>, BoxError> {
        let (name, v, ok) = cut(arg, b'=');
        if ok && name.starts_with(b"-") {
            return Ok(v.to_vec());
        }

        if *i + 1 >= args.len() {
            return Err(errors::new(format!(
                "{} needs a value",
                String::from_utf8_lossy(arg)
            )));
        }

        *i += 1;

        Ok(args[*i].as_ref().to_vec())
    };

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_ref();
        let (name, _, _) = cut(arg, b'=');

        match name {
            b"--url" | b"-url" => r.url = value(arg, &mut i)?,
            b"--user" | b"-user" => r.user = value(arg, &mut i)?,
            b"--token" | b"-token" => r.token = value(arg, &mut i)?,
            b"--output" | b"-output" | b"-o" => r.output = value(arg, &mut i)?,
            b"--no-verify" => r.verify = false,
            _ => {
                return Err(errors::new(format!(
                    "unknown argument {}, usage: elysiandb backup [--output <folder>] [--url <address> [--user <name>] [--token <token>] [--no-verify]]",
                    quote_text(arg)
                )));
            }
        }
        i += 1;
    }

    if r.url.is_empty() {
        return Err(errors::new("--url is empty"));
    }

    Ok(r)
}

fn login_body(user: &[u8], password: &[u8]) -> Vec<u8> {
    let mut body = b"{\"password\":".to_vec();
    append_string(&mut body, password, true);
    body.extend_from_slice(b",\"username\":");
    append_string(&mut body, user, true);
    body.push(b'}');
    body
}

fn url_of(base: &[u8], path: &str) -> Vec<u8> {
    let mut url = base.to_vec();
    url.extend_from_slice(path.as_bytes());
    url
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl RemoteBackup {
    pub(crate) fn run(mut self) -> Result<PathBuf, BoxError> {
        let mut client = Client::new();
        let mut end = self.url.len();
        while end > 0 && self.url[end - 1] == b'/' {
            end -= 1;
        }
        let base = self.url[..end].to_vec();
        let mut basic = false;

        if !self.user.is_empty() {
            if self.password.is_empty()
                && let Some(v) = std::env::var_os(REMOTE_PASSWORD_ENV)
            {
                self.password = v.as_bytes().to_vec();
            }

            if self.password.is_empty() {
                return Err(errors::new(format!(
                    "--user needs a password in the {REMOTE_PASSWORD_ENV} environment variable"
                )));
            }

            let mut login = Request::new("POST", url_of(&base, "/api/security/login"));
            login.set_header("Content-Type", b"application/json".to_vec());
            login.body = login_body(&self.user, &self.password);
            let resp = client.send(login)?;
            let status_code = resp.status_code;
            let status = resp.status.clone();
            drop(resp);

            match status_code {
                200 => {}
                401 | 403 | 429 => {
                    if !self.basic_is_possible(&mut client, &base) {
                        return Err(errors::new(format!(
                            "login refused: {}",
                            String::from_utf8_lossy(&status)
                        )));
                    }
                    basic = true;
                }
                _ => basic = true,
            }
        }

        let mut req = Request::new("GET", url_of(&base, "/api/backup"));
        if basic {
            req.set_basic_auth(&self.user, &self.password);
        }

        if !self.token.is_empty() {
            let mut bearer = b"Bearer ".to_vec();
            bearer.extend_from_slice(&self.token);
            req.set_header("Authorization", bearer);
        }

        let mut resp = client.send(req)?;

        if resp.status_code != 200 {
            let mut detail = Vec::new();
            let _ = (&mut resp.body).take(512).read_to_end(&mut detail);
            return Err(errors::new(format!(
                "the server answered {}: {}",
                String::from_utf8_lossy(&resp.status),
                String::from_utf8_lossy(trim_space(&detail))
            )));
        }

        let mut name = std::ffi::OsString::from(backup::file_name(&Time::now().utc()));
        if let Some(disposition) = resp.header("Content-Disposition")
            && let Ok((_, params)) = parse_media_type(disposition)
            && let Some(filename) = params.get(&b"filename"[..])
            && !filename.is_empty()
        {
            name = filepath::base(Path::new(OsStr::from_bytes(filename)));
        }

        let output = Path::new(OsStr::from_bytes(&self.output));
        let path = filepath::join(output, &name);
        if fs::metadata(&path).is_ok() {
            return Err(errors::new(format!(
                "{} already exists",
                String::from_utf8_lossy(path.as_os_str().as_bytes())
            )));
        }

        let mut prefix = b".".to_vec();
        prefix.extend_from_slice(name.as_bytes());
        prefix.push(b'.');
        let (mut tmp, tmp_path) = backup::create_temp(output, &String::from_utf8_lossy(&prefix))?;
        let _cleanup = RemoveOnDrop(tmp_path.clone());

        let mut buf = vec![0u8; COPY_BUFFER];
        loop {
            let n = match resp.body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    return Err(errors::new(format!("the backup was interrupted: {e}")));
                }
            };
            if let Err(e) = tmp.write_all(&buf[..n]) {
                let err = FsError::new("write", &tmp_path, e);
                return Err(errors::new(format!("the backup was interrupted: {err}")));
            }
        }
        drop(resp);

        tmp.sync_all()
            .map_err(|e| FsError::new("sync", &tmp_path, e))?;

        if self.verify {
            tmp.seek(SeekFrom::Start(0))
                .map_err(|e| FsError::new("seek", &tmp_path, e))?;
            backup::verify(&mut tmp)?;
        }

        backup::close_file(tmp, &tmp_path)?;

        fs::rename(&tmp_path, &path).map_err(|e| FsError::link("rename", &tmp_path, &path, e))?;

        durable::OS.sync_dir(output)?;

        Ok(path)
    }

    fn basic_is_possible(&self, client: &mut Client, base: &[u8]) -> bool {
        let mut req = Request::new("HEAD", url_of(base, "/api/backup"));
        req.set_basic_auth(&self.user, &self.password);
        match client.send(req) {
            Ok(resp) => resp.status_code != 401 && resp.status_code != 403,
            Err(_) => false,
        }
    }
}
