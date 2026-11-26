use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::backup::{self, ArchiveError, IncompleteRestoreError};
use crate::durable::FsError;
use crate::gocompat::errors::BoxError;
use crate::gocompat::json;
use crate::gocompat::strconv;
use crate::httpd::{
    BufReader, ChunkedReader, Handler, HijackConn, MultipartReader, RequestConfig, RequestCtx,
    RequestHeader, STATUS_BAD_REQUEST, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK, is_chunked_error,
};
use crate::kv;
use crate::log;
use crate::transport::http::observe;

const RESTORE_PATH: &[u8] = b"/api/restore";
const BACKUP_PATH: &[u8] = b"/api/backup";
pub const HIDDEN_BODY_HEADER: &str = "X-Elysian-Internal-Body-Length";
const RESTORE_READ_IDLE: Duration = Duration::from_secs(5 * 60);
const BACKUP_WRITE_WINDOW: Duration = Duration::from_secs(60 * 60);

const MSG_UNEXPECTED_END: &str = "unexpected end of the request body";
const MSG_INVALID_CHUNKED: &str = "invalid chunked body";
const MSG_READ_TIMEOUT: &str = "timed out reading the request body";
const MSG_READ_FAILED: &str = "cannot read the request body: ";

fn is_route(uri: &[u8], route: &[u8]) -> bool {
    if !uri.starts_with(route) {
        return false;
    }

    uri.len() == route.len() || uri[route.len()] == b'?'
}

pub fn streaming_routes(h: &mut RequestHeader) -> RequestConfig {
    let uri = h.request_uri().to_vec();
    if h.is_get() && is_route(&uri, BACKUP_PATH) {
        return RequestConfig {
            write_timeout: Some(BACKUP_WRITE_WINDOW),
            ..RequestConfig::default()
        };
    }

    if h.is_post() && is_route(&uri, RESTORE_PATH) {
        h.del(HIDDEN_BODY_HEADER);
        let length = h.content_length();
        if length > 0 || length == -1 {
            h.set(HIDDEN_BODY_HEADER, length.to_string());
            h.set_content_length(0);
        }
    }

    RequestConfig::default()
}

pub fn close_after_unread_body(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        next(ctx);
        if !ctx.request.header.peek(HIDDEN_BODY_HEADER).is_empty() && !ctx.hijacked() {
            ctx.set_connection_close();
        }
    })
}

pub fn restore_controller(ctx: &mut RequestCtx) {
    let hidden = ctx.request.header.peek(HIDDEN_BODY_HEADER).to_vec();
    if !hidden.is_empty()
        && let Ok(length) = strconv::atoi(&hidden)
    {
        restore_from_connection(ctx, length);
        return;
    }

    ctx.response.header.set("Content-Type", "application/json");

    let src = match restore_source(ctx) {
        Ok(src) => src,
        Err(reason) => {
            observe::audit(ctx, "restore rejected", kv!["reason" => reason]);
            send_restore_error(ctx, STATUS_BAD_REQUEST, reason.as_bytes());
            return;
        }
    };

    run_restore(ctx, src);
}

fn chain_has<T: std::error::Error + 'static>(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(e) = current {
        if e.downcast_ref::<T>().is_some() {
            return true;
        }
        current = e.source();
    }

    false
}

fn run_restore<R: Read + Seek + Send>(ctx: &mut RequestCtx, src: R) {
    let start = Instant::now();
    let report = match backup::restore(src) {
        Ok(report) => report,
        Err(err) => {
            let message = err.to_string();
            if !chain_has::<IncompleteRestoreError>(err.as_ref())
                && chain_has::<ArchiveError>(err.as_ref())
            {
                observe::audit(ctx, "restore rejected", kv!["reason" => message]);
                send_restore_error(ctx, STATUS_BAD_REQUEST, message.as_bytes());
                return;
            }

            observe::set_error(ctx, &err);
            send_restore_error(ctx, STATUS_INTERNAL_SERVER_ERROR, message.as_bytes());
            return;
        }
    };

    observe::audit(
        ctx,
        "restore done",
        kv![
            "entities" => report.entities,
            "schemas" => report.schemas,
            "kv_keys" => report.kv_keys,
            "duration_ms" => log::since(start),
        ],
    );

    let mut body: BTreeMap<String, json::RawMessage> = BTreeMap::new();
    body.insert("status".to_string(), raw(&"restore completed".to_string()));
    body.insert("entities".to_string(), raw(&report.entities));
    body.insert("schemas".to_string(), raw(&report.schemas));
    body.insert("kv_keys".to_string(), raw(&report.kv_keys));
    body.insert("kv_keys_expired".to_string(), raw(&report.expired_kv_keys));

    ctx.set_status_code(STATUS_OK);
    ctx.set_body(json::marshal(&body).unwrap_or_default());
}

fn raw<T: json::Encode + ?Sized>(v: &T) -> json::RawMessage {
    json::RawMessage::new(json::marshal(v).unwrap_or_default())
}

struct IdleReader<'a> {
    c: &'a mut HijackConn,
}

impl Read for IdleReader<'_> {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        self.c
            .set_read_deadline(Some(Instant::now() + RESTORE_READ_IDLE));

        self.c.read(p)
    }
}

struct ExactReader<R> {
    r: R,
    left: i64,
}

impl<R: Read> Read for ExactReader<R> {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        if self.left <= 0 || p.is_empty() {
            return Ok(0);
        }

        let want = p
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let n = self.r.read(&mut p[..want])?;
        self.left -= n as i64;
        if n == 0 && self.left > 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }

        Ok(n)
    }
}

fn restore_from_connection(ctx: &mut RequestCtx, length: i64) {
    let content_type = ctx.request.header.content_type().to_vec();
    let boundary = ctx.request.header.multipart_form_boundary().to_vec();

    observe::defer(ctx);
    ctx.hijack_set_no_response(true);
    ctx.hijack(move |ctx: &mut RequestCtx, c: &mut HijackConn| {
        ctx.response.header.set("Content-Type", "application/json");
        ctx.response.header.set_connection_close();

        let received = {
            let idle = IdleReader { c: &mut *c };
            if length < 0 {
                let body = ChunkedReader::new(BufReader::with_capacity(64 << 10, idle));
                receive_archive(body, &content_type, &boundary)
            } else {
                let body = ExactReader {
                    r: idle,
                    left: length,
                };
                receive_archive(body, &content_type, &boundary)
            }
        };

        match received {
            Ok(mut upload) => {
                run_restore(ctx, &mut upload.file);
                upload.discard();
            }
            Err(ReceiveError::Rejected(reason, upload)) => {
                if let Some(upload) = upload {
                    upload.discard();
                }
                observe::audit(ctx, "restore rejected", kv!["reason" => reason]);
                send_restore_error(ctx, STATUS_BAD_REQUEST, reason.as_bytes());
            }
            Err(ReceiveError::Failed(err, upload)) => {
                if let Some(upload) = upload {
                    upload.discard();
                }
                observe::set_error(ctx, &err);
                send_restore_error(
                    ctx,
                    STATUS_INTERNAL_SERVER_ERROR,
                    err.to_string().as_bytes(),
                );
            }
        }

        c.set_write_deadline(Some(Instant::now() + RESTORE_READ_IDLE));
        let mut w = BufWriter::new(&mut *c);
        if ctx.response.write(&mut w).is_ok() {
            let _ = w.flush();
        }
        drop(w);

        observe::finish_deferred(ctx);
    });
}

struct Upload {
    file: File,
    path: PathBuf,
}

impl Upload {
    fn create() -> Result<Upload, BoxError> {
        let (file, path) = backup::new_upload()?;
        Ok(Upload { file, path })
    }

    fn discard(self) {
        drop(self.file);
        let _ = std::fs::remove_file(&self.path);
    }
}

enum ReceiveError {
    Rejected(&'static str, Option<Upload>),
    Failed(BoxError, Option<Upload>),
}

#[derive(Debug)]
struct BodyReadError(String);

impl std::fmt::Display for BodyReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BodyReadError {}

fn body_read_error(e: &io::Error) -> BoxError {
    let message = if is_chunked_error(e) {
        MSG_INVALID_CHUNKED.to_string()
    } else {
        match e.kind() {
            io::ErrorKind::UnexpectedEof => MSG_UNEXPECTED_END.to_string(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => MSG_READ_TIMEOUT.to_string(),
            _ => format!("{MSG_READ_FAILED}{e}"),
        }
    };

    Box::new(BodyReadError(message))
}

enum CopyError {
    Read(io::Error),
    Write(io::Error),
}

fn copy_into(file: &mut File, mut body: impl Read) -> Result<u64, CopyError> {
    let mut buf = vec![0u8; 32 << 10];
    let mut n: u64 = 0;
    loop {
        let read = match body.read(&mut buf) {
            Ok(0) => return Ok(n),
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(CopyError::Read(e)),
        };
        if let Err(e) = file.write_all(&buf[..read]) {
            return Err(CopyError::Write(e));
        }
        n += read as u64;
    }
}

fn copy_error(err: CopyError, upload: &Upload) -> BoxError {
    match err {
        CopyError::Read(e) => body_read_error(&e),
        CopyError::Write(e) => Box::new(FsError::new("write", upload.path.clone(), e)),
    }
}

fn rewind(mut upload: Upload) -> Result<Upload, ReceiveError> {
    if let Err(e) = upload.file.seek(SeekFrom::Start(0)) {
        let err = Box::new(FsError::new("seek", upload.path.clone(), e));
        return Err(ReceiveError::Failed(err, Some(upload)));
    }

    Ok(upload)
}

fn receive_archive(
    body: impl Read,
    content_type: &[u8],
    boundary: &[u8],
) -> Result<Upload, ReceiveError> {
    if !content_type.starts_with(b"multipart/form-data") {
        let mut upload = match Upload::create() {
            Ok(upload) => upload,
            Err(err) => return Err(ReceiveError::Failed(err, None)),
        };

        let n = match copy_into(&mut upload.file, body) {
            Ok(n) => n,
            Err(e) => {
                let err = copy_error(e, &upload);
                return Err(ReceiveError::Failed(err, Some(upload)));
            }
        };

        if n == 0 {
            return Err(ReceiveError::Rejected(
                "the request body must be a backup archive",
                Some(upload),
            ));
        }

        return rewind(upload);
    }

    if boundary.is_empty() {
        return Err(ReceiveError::Rejected("invalid multipart body", None));
    }

    let mut reader = MultipartReader::new(body, boundary);
    let mut files: BTreeMap<Vec<u8>, Upload> = BTreeMap::new();
    let discard_all = |files: BTreeMap<Vec<u8>, Upload>| {
        for (_, upload) in files {
            upload.discard();
        }
    };

    loop {
        let part = match reader.next_part() {
            Ok(Some(part)) => part,
            Ok(None) => break,
            Err(_) => {
                discard_all(files);
                return Err(ReceiveError::Rejected("invalid multipart body", None));
            }
        };

        let field = part.form_name().to_vec();
        if part.file_name().is_empty() {
            continue;
        }

        if files.contains_key(&field) {
            continue;
        }

        let upload = match Upload::create() {
            Ok(upload) => upload,
            Err(err) => {
                discard_all(files);
                return Err(ReceiveError::Failed(err, None));
            }
        };
        files.insert(field.clone(), upload);

        let copied = match files.get_mut(&field) {
            Some(upload) => copy_into(&mut upload.file, reader.part_reader()),
            None => Ok(0),
        };
        if copied.is_err() {
            discard_all(files);
            return Err(ReceiveError::Rejected("invalid multipart body", None));
        }
    }

    let Some(first) = files.keys().next().cloned() else {
        return Err(ReceiveError::Rejected(
            "the multipart body holds no file",
            None,
        ));
    };
    let chosen = files.remove(&first);
    discard_all(files);

    match chosen {
        Some(upload) => rewind(upload),
        None => Err(ReceiveError::Rejected(
            "the multipart body holds no file",
            None,
        )),
    }
}

fn restore_source(ctx: &mut RequestCtx) -> Result<Cursor<Vec<u8>>, &'static str> {
    if !ctx
        .request
        .header
        .content_type()
        .starts_with(b"multipart/form-data")
    {
        let body = ctx.post_body();
        if body.is_empty() {
            return Err("the request body must be a backup archive");
        }

        return Ok(Cursor::new(body.to_vec()));
    }

    let Ok(form) = ctx.multipart_form() else {
        return Err("invalid multipart body");
    };

    for headers in form.file.values() {
        if let Some(header) = headers.first() {
            return Ok(Cursor::new(header.open().into_inner().to_vec()));
        }
    }

    Err("the multipart body holds no file")
}

fn send_restore_error(ctx: &mut RequestCtx, status: u16, message: &[u8]) {
    let body = BTreeMap::from([("error".to_string(), message.to_vec())]);
    let body = json::marshal(&body).unwrap_or_default();
    ctx.set_status_code(status);
    ctx.set_body(body);
}
