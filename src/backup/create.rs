use std::any::Any;
use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::io::IntoRawFd;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::api;
use crate::archive::gzip::{self, BEST_SPEED};
use crate::archive::tar::{self, Header, TYPE_REG};
use crate::crypto::{rand, sha256::Digest};
use crate::datastore::{PointInTime, Snapshot, decode_map};
use crate::durable::{self, Fs, FsError, filepath};
use crate::engine;
use crate::globals;
use crate::gocompat::base64::STD_ENCODING;
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::{self, Map, Value};
use crate::gocompat::time::{self as gotime, RFC3339, Time};
use crate::gocompat::utf8;
use crate::schema::SCHEMA_ENTITY;
use crate::security::{self, ByteString};
use crate::storage;

use super::archive::{
    EntityManifest, FORMAT, FORMAT_VERSION, KEY_PATH, KV_ENCODING_BASE64, KV_PATH, KvEntry,
    MANIFEST_PATH, Manifest, encode_line, entity_path, file_name, hex, schema_path,
};
use super::status::begin;

const BEAT_EVERY: Duration = Duration::from_secs(1);
const MAX_READ_PAUSE: Duration = Duration::from_millis(100);

const ENTRY_BUFFER: usize = 64 << 10;
const SPOOL_BUFFER: usize = 4096;
const COPY_BUFFER: usize = 32 << 10;
const TEMP_TRIES: usize = 10000;

const UNKNOWN_PANIC: &str = "unknown panic";

#[derive(Default)]
pub struct Options<'a> {
    pub beat: Option<&'a mut (dyn FnMut() + Send)>,
    pub flush: bool,
    pub pace: bool,
}

enum Body {
    Text(Vec<u8>),
    Schema(Map),
    Entity { slot: usize, snap: Option<Snapshot> },
    Keys,
    Spooled(PathBuf),
}

struct Entry {
    name: String,
    body: Body,
    size: i64,
    measured: bool,
}

impl Entry {
    fn new(name: String, body: Body) -> Entry {
        Entry {
            name,
            body,
            size: 0,
            measured: false,
        }
    }
}

pub struct Source {
    now: Time,
    manifest: Manifest,
    entries: Vec<Entry>,
    point: Option<PointInTime>,
    spool: Option<PathBuf>,
    closed: bool,
}

pub fn prepare(now: Time) -> Result<Source, BoxError> {
    let manifest = Manifest {
        format: FORMAT.to_string(),
        format_version: FORMAT_VERSION,
        elysiandb_version: globals::VERSION.to_string(),
        created_at: now.utc().format(RFC3339),
        engine: globals::engine(),
        entities: Vec::new(),
        schemas: Vec::new(),
        kv_keys: 0,
        files: BTreeMap::new(),
    };

    let key = security::create_key_file_or_get_key()?;

    let mut s = Source {
        now,
        manifest,
        entries: vec![Entry::new(KEY_PATH.to_string(), Body::Text(key))],
        point: None,
        spool: None,
        closed: false,
    };

    let Some(e) = storage::engine() else {
        if let Err(err) = s.spool() {
            s.close();
            return Err(err);
        }

        return Ok(s);
    };

    s.point = Some(e.point_in_time());

    if let Err(err) = s.add_schemas() {
        s.close();
        return Err(err);
    }

    s.add_entities();
    s.add_keys();

    Ok(s)
}

impl Source {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;

        if let Some(point) = &self.point {
            point.release();
        }

        if let Some(folder) = self.spool.take() {
            let _ = fs::remove_dir_all(folder);
        }
    }

    fn add_schemas(&mut self) -> Result<(), BoxError> {
        let Some(point) = &self.point else {
            return Ok(());
        };

        let Some(snap) = point.snapshots.get(SCHEMA_ENTITY.as_bytes()).cloned() else {
            return Ok(());
        };

        let manifest = &mut self.manifest;
        let entries = &mut self.entries;
        api::iterate_snapshot(&snap, &mut |data: Map| {
            let Some(entity) = string_id(&data).map(<[u8]>::to_vec) else {
                return Ok(());
            };

            manifest.schemas.push(ByteString(entity.clone()));
            entries.push(Entry::new(schema_path(&entity), Body::Schema(data)));

            Ok(())
        })
    }

    fn add_entities(&mut self) {
        let Some(point) = &self.point else {
            return;
        };

        let mut seen: HashSet<&[u8]> = HashSet::new();
        for name in &point.types {
            if name.as_slice() == SCHEMA_ENTITY.as_bytes() || !seen.insert(name.as_slice()) {
                continue;
            }

            self.manifest.entities.push(EntityManifest {
                name: ByteString(name.clone()),
                documents: 0,
            });
            let slot = self.manifest.entities.len() - 1;
            let snap = point.snapshots.get(name).cloned();

            self.entries
                .push(Entry::new(entity_path(name), Body::Entity { slot, snap }));
        }
    }

    fn add_keys(&mut self) {
        self.entries
            .push(Entry::new(KV_PATH.to_string(), Body::Keys));
    }

    pub fn write_archive<W: Write>(&mut self, w: W, mut opts: Options<'_>) -> Result<(), BoxError> {
        let done = begin();
        let counted = Cell::new(0i64);

        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.write_counted(Counting { w, n: &counted }, &mut opts)
        }));

        let result = match outcome {
            Ok(result) => result,
            Err(payload) => Err(errors::new(format!(
                "the backup stopped: {}",
                panic_message(payload.as_ref())
            ))),
        };

        done(&self.manifest, counted.get(), result.as_ref().err());

        result
    }

    fn write_counted<W: Write>(
        &mut self,
        counted: Counting<'_, W>,
        opts: &mut Options<'_>,
    ) -> Result<(), BoxError> {
        let mut gz = gzip::Writer::new_level(counted, BEST_SPEED)?;

        let Source {
            now,
            manifest,
            entries,
            point,
            ..
        } = self;
        let point = point.as_ref();
        let pace = opts.pace;
        let flush_gzip = opts.flush || opts.beat.is_some();
        let mut beat = Beat {
            last: Instant::now(),
            opts,
        };

        for e in entries.iter_mut() {
            if e.measured {
                continue;
            }

            let mut beats = || {
                if !beat.due() {
                    return;
                }

                if flush_gzip {
                    let _ = gz.flush();
                    if beat.opts.flush {
                        let _ = gz.get_mut().flush();
                    }
                }

                beat.call();
            };

            let (sum, size) = run(&e.body, point, manifest, None, &mut beats, pace)?;

            e.size = size;
            manifest.files.insert(e.name.clone(), sum);
        }

        let mut tw = tar::Writer::new(gz);
        let mod_time = now.utc().truncate(gotime::Duration::SECOND);

        let mut manifest_data = json::marshal_indent(&*manifest, "", "  ")?;
        manifest_data.push(b'\n');

        write_entry_header(
            &mut tw,
            MANIFEST_PATH,
            manifest_data.len() as i64,
            &mod_time,
        )?;

        tw.write_all(&manifest_data)?;

        for e in entries.iter() {
            write_entry_header(&mut tw, &e.name, e.size, &mod_time)?;

            let mut beats = || {
                if beat.due() {
                    beat.call();
                }
            };

            let (sum, _) = run(
                &e.body,
                point,
                manifest,
                Some(&mut tw as &mut dyn Write),
                &mut beats,
                pace,
            )?;

            if manifest.files.get(&e.name).map(String::as_str) != Some(sum.as_str()) {
                return Err(errors::new(format!(
                    "{} changed while the backup was read",
                    e.name
                )));
            }
        }

        tw.close()?;

        let mut gz = tw.into_inner();
        gz.close()?;

        Ok(())
    }

    fn spool(&mut self) -> Result<(), BoxError> {
        let folder = mkdir_temp(&std::env::temp_dir(), "elysiandb-backup-")?;
        self.spool = Some(folder.clone());

        let mut entries = std::mem::take(&mut self.entries);
        let key = entries.remove(0);
        let Body::Text(key_bytes) = key.body else {
            return Ok(());
        };

        let manifest = &mut self.manifest;
        let entries = &mut self.entries;

        spool_entry(
            &folder,
            key.name,
            manifest,
            entries,
            &mut |w: &mut dyn Write| {
                w.write_all(&key_bytes)?;
                Ok(())
            },
        )?;

        let types = engine::list_entity_types();

        engine::iterate_entities(SCHEMA_ENTITY.as_bytes(), &mut |data: Map| {
            let Some(entity) = string_id(&data).map(<[u8]>::to_vec) else {
                return Ok(());
            };

            manifest.schemas.push(ByteString(entity.clone()));

            spool_entry(
                &folder,
                schema_path(&entity),
                manifest,
                entries,
                &mut |w: &mut dyn Write| encode_line(w, &data),
            )
        })?;

        let mut seen: HashSet<&[u8]> = HashSet::new();
        for entity in &types {
            if entity.as_slice() == SCHEMA_ENTITY.as_bytes() || !seen.insert(entity.as_slice()) {
                continue;
            }

            let mut count = 0i64;

            spool_entry(
                &folder,
                entity_path(entity),
                manifest,
                entries,
                &mut |w: &mut dyn Write| {
                    engine::iterate_entities(entity, &mut |data: Map| {
                        if string_id(&data).is_none() {
                            return Ok(());
                        }

                        count += 1;
                        encode_line(w, &data)
                    })
                },
            )?;

            manifest.entities.push(EntityManifest {
                name: ByteString(entity.clone()),
                documents: count,
            });
        }

        let mut kv_keys = 0i64;
        let result = spool_entry(
            &folder,
            KV_PATH.to_string(),
            manifest,
            entries,
            &mut |w: &mut dyn Write| {
                for key in storage::list_keys() {
                    if globals::is_api_key(&key) || storage::key_has_expired(&key) {
                        continue;
                    }

                    let Ok(value) = storage::get_by_key(&key) else {
                        continue;
                    };

                    let mut kv = new_kv_entry(&key, &value);
                    if let Some(expires_at) = storage::get_key_expiration(&key) {
                        kv.expires_at = expires_at;
                    }

                    encode_line(w, &kv)?;

                    kv_keys += 1;
                }

                Ok(())
            },
        );
        manifest.kv_keys += kv_keys;

        result
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn create<W: Write>(w: W, now: Time) -> Result<Manifest, BoxError> {
    let mut src = prepare(now)?;

    let result = src.write_archive(w, Options::default());
    src.close();
    result?;

    Ok(src.manifest.clone())
}

pub fn write_file(folder: &Path, now: Time) -> Result<(PathBuf, Manifest), BoxError> {
    write_file_with(folder, now, Options::default())
}

pub(super) fn write_file_with(
    folder: &Path,
    now: Time,
    opts: Options<'_>,
) -> Result<(PathBuf, Manifest), BoxError> {
    let name = file_name(&now);
    let path = filepath::join(folder, &name);
    if fs::metadata(&path).is_ok() {
        return Err(errors::new(format!("{} already exists", path_text(&path))));
    }

    let (mut tmp, tmp_path) = create_temp(folder, &format!(".{name}."))?;
    let _cleanup = RemoveOnDrop(tmp_path.clone());

    let mut src = match prepare(now) {
        Ok(src) => src,
        Err(err) => {
            drop(tmp);
            return Err(err);
        }
    };

    let mut err = src.write_archive(&mut tmp, opts).err();
    if err.is_none() {
        err = tmp
            .sync_all()
            .map_err(|e| Box::new(FsError::new("sync", &tmp_path, e)) as BoxError)
            .err();
    }

    if let Err(close_err) = close_file(tmp, &tmp_path)
        && err.is_none()
    {
        err = Some(Box::new(close_err));
    }

    if let Some(err) = err {
        return Err(err);
    }

    fs::rename(&tmp_path, &path).map_err(|e| FsError::link("rename", &tmp_path, &path, e))?;

    durable::OS.sync_dir(folder)?;

    let manifest = src.manifest.clone();
    src.close();

    Ok((path, manifest))
}

fn string_id(data: &Map) -> Option<&[u8]> {
    match data.get("id") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.as_bytes()),
        _ => None,
    }
}

fn spool_entry(
    folder: &Path,
    name: String,
    manifest: &mut Manifest,
    entries: &mut Vec<Entry>,
    write: &mut dyn FnMut(&mut dyn Write) -> Result<(), BoxError>,
) -> Result<(), BoxError> {
    let (file, path) = create_temp(folder, "entry-")?;

    let mut hashed = HashedFile {
        file: &file,
        path: &path,
        hash: Digest::new(),
    };

    let mut err = {
        let mut buffered = GoBuffer::new(&mut hashed, SPOOL_BUFFER);
        let mut err = write(&mut buffered).err();
        if err.is_none() {
            err = buffered
                .flush_buffer()
                .err()
                .map(|e| Box::new(e) as BoxError);
        }
        err
    };
    let sum = hex(&hashed.hash.sum());

    if let Err(close_err) = close_file(file, &path)
        && err.is_none()
    {
        err = Some(Box::new(close_err));
    }

    if let Some(err) = err {
        return Err(err);
    }

    let size = fs::metadata(&path)
        .map_err(|e| FsError::new("stat", &path, e))?
        .len() as i64;

    manifest.files.insert(name.clone(), sum);
    entries.push(Entry {
        name,
        body: Body::Spooled(path),
        size,
        measured: true,
    });

    Ok(())
}

fn run(
    body: &Body,
    point: Option<&PointInTime>,
    manifest: &mut Manifest,
    out: Option<&mut dyn Write>,
    beats: &mut dyn FnMut(),
    pace: bool,
) -> Result<(String, i64), BoxError> {
    let mut sum = SumWriter {
        hash: Digest::new(),
        out,
        size: 0,
        beats,
    };

    {
        let mut buffered = GoBuffer::new(&mut sum, ENTRY_BUFFER);
        write_body(body, point, manifest, &mut buffered, pace)?;
        buffered.flush_buffer()?;
    }

    Ok((hex(&sum.hash.sum()), sum.size))
}

fn write_body(
    body: &Body,
    point: Option<&PointInTime>,
    manifest: &mut Manifest,
    w: &mut dyn Write,
    pace: bool,
) -> Result<(), BoxError> {
    match body {
        Body::Text(b) => {
            w.write_all(b)?;
            Ok(())
        }
        Body::Schema(data) => encode_line(w, data),
        Body::Entity { slot, snap } => {
            let Some(snap) = snap else {
                return Ok(());
            };

            let (count, result) = write_documents(w, snap, pace);
            manifest.entities[*slot].documents = count;

            result
        }
        Body::Keys => match point {
            Some(point) => write_keys(w, point, manifest),
            None => Ok(()),
        },
        Body::Spooled(path) => copy_file(path, w),
    }
}

fn write_keys(
    w: &mut dyn Write,
    point: &PointInTime,
    manifest: &mut Manifest,
) -> Result<(), BoxError> {
    let now = point.at.unix();
    let mut count = 0i64;
    for item in &point.kv {
        if globals::is_api_key(&item.key) || (item.expires_at > 0 && now >= item.expires_at) {
            continue;
        }

        let mut kv = new_kv_entry(&item.key, item.value.as_deref().unwrap_or(&[]));
        if item.expires_at > 0 {
            kv.expires_at = item.expires_at;
        }

        encode_line(w, &kv)?;

        count += 1;
    }
    manifest.kv_keys = count;

    Ok(())
}

fn copy_file(path: &Path, w: &mut dyn Write) -> Result<(), BoxError> {
    let mut f = File::open(path).map_err(|e| FsError::new("open", path, e))?;
    let mut buf = vec![0u8; COPY_BUFFER];
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Box::new(FsError::new("read", path, e))),
        };

        w.write_all(&buf[..n])?;
    }
}

const ESCAPED_BY_THE_STORE: &[u8] = b"\\u";

fn write_documents(w: &mut dyn Write, snap: &Snapshot, pace: bool) -> (i64, Result<(), BoxError>) {
    let mut count = 0i64;
    let mut failed: Option<BoxError> = None;

    let pause: Box<dyn FnMut(Duration) + Send> = Box::new(move |took: Duration| {
        if pace {
            std::thread::sleep(took.min(MAX_READ_PAUSE));
        }
    });

    snap.scan_values_paced(Some(pause), |_, raw| {
        let raw = match raw {
            Ok(raw) => raw,
            Err(err) => {
                failed = Some(err);
                return false;
            }
        };

        if !json::valid(raw) {
            return true;
        }

        if let Some(id) = simple_document_id(raw)
            && !id.is_empty()
            && !contains(raw, ESCAPED_BY_THE_STORE)
        {
            count += 1;
            if let Err(err) = w.write_all(raw) {
                failed = Some(Box::new(err));
                return false;
            }

            if let Err(err) = w.write_all(b"\n") {
                failed = Some(Box::new(err));
                return false;
            }

            return true;
        }

        let data = match decode_map(raw) {
            Ok(Some(data)) => data,
            _ => return true,
        };

        if string_id(&data).is_none() {
            return true;
        }

        count += 1;
        if let Err(err) = encode_line(w, &data) {
            failed = Some(err);
            return false;
        }

        true
    });

    (count, failed.map_or(Ok(()), Err))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

pub(super) fn simple_document_id(raw: &[u8]) -> Option<&[u8]> {
    let mut i = skip_space(raw, 0);
    if i >= raw.len() || raw[i] != b'{' {
        return None;
    }
    i += 1;

    let mut id: &[u8] = b"";
    let mut found = false;
    loop {
        i = skip_space(raw, i);
        if i >= raw.len() {
            return None;
        }

        if raw[i] == b'}' {
            return if found { Some(id) } else { None };
        }

        if raw[i] == b',' {
            i += 1;
            continue;
        }

        let (key_start, key_end, next, plain) = scan_string(raw, i)?;
        if !plain {
            return None;
        }

        i = skip_space(raw, next);
        if i >= raw.len() || raw[i] != b':' {
            return None;
        }
        i = skip_space(raw, i + 1);

        if &raw[key_start..key_end] == b"id" {
            if i >= raw.len() || raw[i] != b'"' {
                return None;
            }

            let (val_start, val_end, after, val_plain) = scan_string(raw, i)?;
            if !val_plain {
                return None;
            }

            id = &raw[val_start..val_end];
            found = true;
            i = after;
            continue;
        }

        i = skip_value(raw, i)?;
    }
}

fn skip_space(raw: &[u8], mut i: usize) -> usize {
    while i < raw.len() && matches!(raw[i], b' ' | b'\n' | b'\t' | b'\r') {
        i += 1;
    }

    i
}

fn scan_string(raw: &[u8], i: usize) -> Option<(usize, usize, usize, bool)> {
    if i >= raw.len() || raw[i] != b'"' {
        return None;
    }

    let start = i + 1;
    let mut plain = true;
    let mut j = start;
    while j < raw.len() {
        match raw[j] {
            b'\\' => {
                plain = false;
                j += 1;
            }
            b'"' => return Some((start, j, j + 1, plain)),
            _ => {}
        }
        j += 1;
    }

    None
}

fn skip_value(raw: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 0usize;
    while i < raw.len() {
        match raw[i] {
            b'"' => {
                let (_, _, next, _) = scan_string(raw, i)?;
                i = next;
                if depth == 0 {
                    return Some(i);
                }
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            b',' => {
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

pub(super) fn new_kv_entry(key: &[u8], value: &[u8]) -> KvEntry {
    if utf8::valid(value) {
        return KvEntry {
            key: ByteString(key.to_vec()),
            value: ByteString(value.to_vec()),
            ..KvEntry::default()
        };
    }

    KvEntry {
        key: ByteString(key.to_vec()),
        value: ByteString(STD_ENCODING.encode_to_string(value).into_bytes()),
        encoding: KV_ENCODING_BASE64.to_string(),
        expires_at: 0,
    }
}

struct Beat<'o, 'a> {
    last: Instant,
    opts: &'o mut Options<'a>,
}

impl Beat<'_, '_> {
    fn due(&mut self) -> bool {
        if self.last.elapsed() < BEAT_EVERY {
            return false;
        }
        self.last = Instant::now();

        true
    }

    fn call(&mut self) {
        if let Some(beat) = self.opts.beat.as_deref_mut() {
            beat();
        }
    }
}

struct Counting<'c, W> {
    w: W,
    n: &'c Cell<i64>,
}

impl<W: Write> Write for Counting<'_, W> {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        let n = self.w.write(p)?;
        self.n.set(self.n.get() + n as i64);

        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

struct SumWriter<'o, 'b> {
    hash: Digest,
    out: Option<&'o mut dyn Write>,
    size: i64,
    beats: &'b mut dyn FnMut(),
}

impl Write for SumWriter<'_, '_> {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.size += p.len() as i64;
        self.hash.write(p);
        (self.beats)();

        match self.out.as_mut() {
            None => Ok(p.len()),
            Some(out) => {
                out.write_all(p)?;
                Ok(p.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct HashedFile<'a> {
    file: &'a File,
    path: &'a Path,
    hash: Digest,
}

impl Write for HashedFile<'_> {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        let mut file = self.file;
        if let Err(e) = file.write_all(p) {
            return Err(io::Error::new(
                e.kind(),
                FsError::new("write", self.path, e),
            ));
        }
        self.hash.write(p);

        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct GoBuffer<'a> {
    w: &'a mut dyn Write,
    buf: Vec<u8>,
    size: usize,
    err: Option<(io::ErrorKind, String)>,
}

impl<'a> GoBuffer<'a> {
    fn new(w: &'a mut dyn Write, size: usize) -> GoBuffer<'a> {
        GoBuffer {
            w,
            buf: Vec::with_capacity(size),
            size,
            err: None,
        }
    }

    fn available(&self) -> usize {
        self.size - self.buf.len()
    }

    fn sticky(&self) -> io::Result<()> {
        match &self.err {
            Some((kind, msg)) => Err(io::Error::new(*kind, msg.clone())),
            None => Ok(()),
        }
    }

    fn fail(&mut self, e: io::Error) -> io::Error {
        self.err = Some((e.kind(), e.to_string()));
        e
    }

    fn flush_buffer(&mut self) -> io::Result<()> {
        self.sticky()?;
        if self.buf.is_empty() {
            return Ok(());
        }

        if let Err(e) = self.w.write_all(&self.buf) {
            return Err(self.fail(e));
        }
        self.buf.clear();

        Ok(())
    }
}

impl Write for GoBuffer<'_> {
    fn write(&mut self, mut p: &[u8]) -> io::Result<usize> {
        let mut nn = 0;
        while p.len() > self.available() {
            self.sticky()?;

            let n = if self.buf.is_empty() {
                if let Err(e) = self.w.write_all(p) {
                    return Err(self.fail(e));
                }
                p.len()
            } else {
                let n = self.available();
                self.buf.extend_from_slice(&p[..n]);
                self.flush_buffer()?;
                n
            };

            nn += n;
            p = &p[n..];
        }

        self.sticky()?;
        self.buf.extend_from_slice(p);

        Ok(nn + p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_buffer()
    }
}

fn write_entry_header<W: Write>(
    tw: &mut tar::Writer<W>,
    name: &str,
    size: i64,
    mod_time: &Time,
) -> io::Result<()> {
    tw.write_header(&Header {
        name: name.as_bytes().to_vec(),
        mode: 0o600,
        size,
        mod_time: mod_time.clone(),
        typeflag: TYPE_REG,
        ..Header::default()
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }

    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }

    UNKNOWN_PANIC.to_string()
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(path.as_os_str().as_bytes()).into_owned()
}

pub(super) fn close_file(file: File, path: &Path) -> Result<(), FsError> {
    let fd = file.into_raw_fd();
    if unsafe { libc::close(fd) } != 0 {
        return Err(FsError::new("close", path, io::Error::last_os_error()));
    }

    Ok(())
}

fn join_path(dir: &Path, name: &str) -> Vec<u8> {
    let mut out = dir.as_os_str().as_bytes().to_vec();
    if out.last() != Some(&b'/') {
        out.push(b'/');
    }
    out.extend_from_slice(name.as_bytes());

    out
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

fn temp_folder(dir: &Path) -> PathBuf {
    if dir.as_os_str().is_empty() {
        return std::env::temp_dir();
    }

    dir.to_path_buf()
}

pub(super) fn create_temp(dir: &Path, prefix: &str) -> Result<(File, PathBuf), FsError> {
    let base = join_path(&temp_folder(dir), prefix);

    let mut tries = 0;
    loop {
        let mut name = base.clone();
        name.extend_from_slice(next_random().as_bytes());
        let name = PathBuf::from(OsString::from_vec(name));

        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&name)
        {
            Ok(file) => return Ok((file, name)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                tries += 1;
                if tries < TEMP_TRIES {
                    continue;
                }

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

fn mkdir_temp(dir: &Path, prefix: &str) -> Result<PathBuf, FsError> {
    let dir = temp_folder(dir);
    let base = join_path(&dir, prefix);

    let mut tries = 0;
    loop {
        let mut name = base.clone();
        name.extend_from_slice(next_random().as_bytes());
        let name = PathBuf::from(OsString::from_vec(name));

        match fs::DirBuilder::new().mode(0o700).create(&name) {
            Ok(()) => return Ok(name),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                tries += 1;
                if tries < TEMP_TRIES {
                    continue;
                }

                let mut pattern = base.clone();
                pattern.push(b'*');
                return Err(FsError::new(
                    "mkdir",
                    PathBuf::from(OsString::from_vec(pattern)),
                    e,
                ));
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound
                    && let Err(stat_err) = fs::metadata(&dir)
                    && stat_err.kind() == io::ErrorKind::NotFound
                {
                    return Err(FsError::new("stat", &dir, stat_err));
                }

                return Err(FsError::new("mkdir", name, e));
            }
        }
    }
}
