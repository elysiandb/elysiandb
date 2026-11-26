use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::acl::{self, ReadMode};
use crate::api::{self, ListResult};
use crate::cache;
use crate::crypto::rand;
use crate::datastore::Doc;
use crate::durable::FsError;
use crate::globals;
use crate::gocompat::errors::BoxError;
use crate::httpd::{RequestCtx, STATUS_OK, STATUS_SERVICE_UNAVAILABLE};
use crate::security;

use super::guard::{request_guard, send_guard_error};

const STREAM_THRESHOLD: usize = 1 << 20;

const DEFAULT_SPOOL_MB: i64 = 8;

const TEMP_TRIES: usize = 10_000;

pub fn send_deadline_exceeded(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_SERVICE_UNAVAILABLE);
    ctx.set_body_string(r#"{"error":"request deadline exceeded"}"#);
}

pub fn count_planned(res: &ListResult<'_>, paged: bool, mode: ReadMode, username: &[u8]) -> usize {
    match mode {
        ReadMode::ReadNone => return 0,
        ReadMode::ReadOwned => {
            let mut n = 0;
            for d in &res.docs {
                if let Ok(doc) = d.decode()
                    && acl::owned_by(&doc, username)
                {
                    n += 1;
                }
            }
            return n;
        }
        ReadMode::ReadAll => {}
    }

    if paged {
        return res.docs.len();
    }

    res.total
}

pub fn cache_deps(entity: &[u8], includes: &[u8]) -> Vec<Vec<u8>> {
    api::cache_dependencies(entity, includes, security::user_authentication_is_enabled())
}

pub(crate) fn cache_enabled() -> bool {
    globals::config().is_some_and(|cfg| cfg.api.cache.enabled)
}

pub(crate) fn store_in_cache(entity: &[u8], hash: Option<&[u8]>, body: &[u8], deps: &[Vec<u8>]) {
    if !cache_enabled() {
        return;
    }

    if let (Some(hash), Some(store)) = (hash, cache::cache_store()) {
        store.set_with_deps(entity, hash, body.to_vec(), deps);
    }
}

pub fn write_count(
    ctx: &mut RequestCtx,
    count: usize,
    cache_entity: &[u8],
    hash: Option<&[u8]>,
    deps: &[Vec<u8>],
) {
    let response = format!(r#"{{"count":{count}}}"#).into_bytes();

    ctx.response.header.set("Content-Type", "application/json");
    ctx.response.header.set("X-Elysian-Cache", "MISS");
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(&response);

    store_in_cache(cache_entity, hash, &response, deps);
}

type DocumentParts = (Vec<Arc<[u8]>>, usize);

fn document_bytes(docs: &[Doc]) -> Result<DocumentParts, BoxError> {
    let mut parts = Vec::with_capacity(docs.len());
    let mut size = 2;
    for d in docs {
        let b = d.bytes()?;
        size += b.len() + 1;
        parts.push(b);
    }

    Ok((parts, size))
}

pub fn write_documents(
    ctx: &mut RequestCtx,
    docs: &[Doc],
    cache_entity: &[u8],
    hash: Option<&[u8]>,
    cache_header: bool,
    deps: &[Vec<u8>],
) -> Result<(), BoxError> {
    let (parts, size) = document_bytes(docs)?;

    if let Err(err) = request_guard(ctx).charge(i64::try_from(size).unwrap_or(i64::MAX)) {
        send_guard_error(ctx, err);
        return Ok(());
    }

    ctx.response.header.set("Content-Type", "application/json");
    if cache_header {
        ctx.response.header.set("X-Elysian-Cache", "MISS");
    }
    ctx.set_status_code(STATUS_OK);

    let store = cache::cache_store();
    let cacheable =
        cache_enabled() && hash.is_some() && store.as_ref().is_some_and(|s| s.accepts(size));
    if !cacheable && size > STREAM_THRESHOLD {
        ctx.set_body_stream_writer(move |w| {
            let _ = w.write_all(b"[");
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    let _ = w.write_all(b",");
                }
                let _ = w.write_all(p);
                if i % 64 == 63 {
                    let _ = w.flush();
                }
            }
            let _ = w.write_all(b"]");
        });

        return Ok(());
    }

    let mut body = Vec::with_capacity(size);
    body.push(b'[');
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            body.push(b',');
        }
        body.extend_from_slice(p);
    }
    body.push(b']');

    ctx.set_body(&body);
    if cacheable && let (Some(hash), Some(store)) = (hash, store) {
        store.set_with_deps(cache_entity, hash, body, deps);
    }

    Ok(())
}

fn spool_limit() -> usize {
    if let Some(cfg) = globals::config()
        && cfg.api.response.spool_mb > 0
    {
        return usize::try_from(cfg.api.response.spool_mb.wrapping_shl(20)).unwrap_or(usize::MAX);
    }

    usize::try_from(DEFAULT_SPOOL_MB << 20).unwrap_or(usize::MAX)
}

fn next_random() -> String {
    let mut b = [0u8; 4];
    if rand::os_random(&mut b).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        b = nanos.to_le_bytes();
    }

    u32::from_le_bytes(b).to_string()
}

fn create_temp(prefix: &str) -> Result<(File, PathBuf), FsError> {
    let mut base = std::env::temp_dir().into_os_string().into_vec();
    if base.last() != Some(&b'/') {
        base.push(b'/');
    }
    base.extend_from_slice(prefix.as_bytes());

    let mut tries = 0;
    loop {
        let mut name = base.clone();
        name.extend_from_slice(next_random().as_bytes());
        let name = PathBuf::from(OsString::from_vec(name));

        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&name)
        {
            Ok(file) => return Ok((file, name)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && tries + 1 < TEMP_TRIES => {
                tries += 1;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let mut pattern = base.clone();
                pattern.push(b'*');
                return Err(FsError::new(
                    "open",
                    PathBuf::from(OsString::from_vec(pattern)),
                    e,
                ));
            }
            Err(e) => return Err(FsError::new("open", name, e)),
        }
    }
}

pub struct Spool {
    buf: Vec<u8>,
    file: Option<(File, PathBuf)>,
    size: usize,
    limit: usize,
}

impl Spool {
    fn new(limit: usize) -> Spool {
        Spool {
            buf: Vec::new(),
            file: None,
            size: 0,
            limit,
        }
    }

    fn write(&mut self, p: &[u8]) -> Result<(), BoxError> {
        self.size += p.len();
        if self.file.is_none() && self.buf.len() + p.len() > self.limit {
            let (mut f, name) = create_temp("elysiandb-response-")?;

            let _ = std::fs::remove_file(&name);
            if let Err(e) = f.write_all(&self.buf) {
                return Err(Box::new(FsError::new("write", name, e)));
            }

            self.file = Some((f, name));
            self.buf = Vec::new();
        }

        if let Some((f, name)) = &mut self.file {
            if let Err(e) = f.write_all(p) {
                return Err(Box::new(FsError::new("write", name.clone(), e)));
            }
            return Ok(());
        }

        self.buf.extend_from_slice(p);

        Ok(())
    }

    pub fn in_memory(&self) -> Option<&[u8]> {
        match self.file {
            None => Some(&self.buf),
            Some(_) => None,
        }
    }

    pub fn send(self, ctx: &mut RequestCtx) -> Result<(), BoxError> {
        let Some((mut f, name)) = self.file else {
            return Ok(());
        };

        if let Err(e) = f.seek(SeekFrom::Start(0)) {
            return Err(Box::new(FsError::new("seek", name, e)));
        }

        ctx.set_body_stream(Box::new(f), i64::try_from(self.size).unwrap_or(i64::MAX));

        Ok(())
    }
}

pub fn encode_list<I>(items: I) -> Result<Spool, BoxError>
where
    I: IntoIterator<Item = Result<Vec<u8>, BoxError>>,
{
    let mut s = Spool::new(spool_limit());
    s.write(b"[")?;

    for (i, item) in items.into_iter().enumerate() {
        let b = item?;

        if i > 0 {
            s.write(b",")?;
        }

        s.write(&b)?;
    }

    s.write(b"]")?;

    Ok(s)
}
