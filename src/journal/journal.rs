use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::error::Error as StdError;
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::fs::{File, Fs};
use super::record::{
    BATCH_MAGIC, Error, Location, Position, Record, SEGMENT_HEADER_SIZE, append_batch, batch_size,
    check_segment_header, decode_batch, segment_header,
};
use crate::durable::{self, FsError, OpenFlags, filepath};
use crate::gocompat::errors::{self as goerrors, BoxError};
use crate::gocompat::{sort, strconv};
use crate::sync::{Condvar, Mutex, RwLock, Signal, Stop, Ticker, spawn};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Background,
    Write,
    Sync,
}

pub const DEFAULT_SEGMENT_SIZE: i64 = 64 << 20;
const DEFAULT_MAX_PENDING: usize = 16 << 20;
const RETIRE_GRACE: Duration = Duration::from_secs(2 * 60);
const SEGMENT_SUFFIX: &str = ".seg";
const DAMAGED_SUFFIX: &str = ".damaged";
const CHUNK_BUFFER_SIZE: usize = 64 << 10;

pub type WriteErrorHook = Arc<dyn Fn(&(dyn StdError + Send + Sync + 'static)) + Send + Sync>;

#[derive(Clone, Default)]
pub struct Options {
    pub mode: Mode,
    pub segment_size: i64,
    pub retire_grace: Duration,
    pub flush_interval: Duration,
    pub max_pending: usize,
    pub fs: Option<Arc<dyn Fs>>,
    pub on_write_error: Option<WriteErrorHook>,
    pub sync_bytes: i64,
}

#[derive(Clone)]
struct Chunk {
    segment: u32,
    offset: u32,
    data: Vec<u8>,
    cap: usize,
    fresh: bool,
}

struct RetiredFile {
    id: u32,
    file: Arc<dyn File>,
    since: Instant,
}

struct State {
    next_lsn: u64,
    active: u32,
    active_size: u32,
    pending: Vec<Arc<Chunk>>,
    inflight: Vec<Arc<Chunk>>,
    appended: u64,
    written: u64,
    pending_len: usize,
    write_err: Option<Arc<dyn StdError + Send + Sync>>,
    attempts: u64,
    closed: bool,
}

#[derive(Default)]
struct Files {
    files: HashMap<u32, Arc<dyn File>>,
    dirty: HashSet<u32>,
    retired: Vec<RetiredFile>,
}

pub struct Journal {
    dir: PathBuf,
    opts: Options,
    fs: Arc<dyn Fs>,

    mu: Mutex<State>,
    done: Condvar,
    started: AtomicBool,
    flushed: AtomicU64,

    kick: Signal,
    stop: Stop,
    threads: Mutex<Vec<JoinHandle<()>>>,

    holds: AtomicI32,
    sync_mu: Mutex<()>,
    sync_kick: Signal,
    unsynced: AtomicU64,

    files: RwLock<Files>,
}

#[derive(Clone, Default)]
pub struct Ticket {
    j: Option<Arc<Journal>>,
    at: u64,
}

struct SharedError(Arc<dyn StdError + Send + Sync>);

impl fmt::Display for SharedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&*self.0, f)
    }
}

impl fmt::Debug for SharedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl StdError for SharedError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&*self.0)
    }
}

fn shared(err: &Arc<dyn StdError + Send + Sync>) -> BoxError {
    Box::new(SharedError(err.clone()))
}

pub fn segment_name(id: u32) -> String {
    format!("{id:06}{SEGMENT_SUFFIX}")
}

pub fn open(dir: &Path, mut opts: Options) -> Result<Arc<Journal>, FsError> {
    let fs = durable::or(opts.fs.take());
    opts.fs = Some(fs.clone());

    if opts.segment_size <= 0 {
        opts.segment_size = DEFAULT_SEGMENT_SIZE;
    }
    if opts.retire_grace.is_zero() {
        opts.retire_grace = RETIRE_GRACE;
    }

    if opts.max_pending == 0 {
        opts.max_pending = DEFAULT_MAX_PENDING;
    }

    fs.mkdir_all(dir, 0o700)?;

    Ok(Arc::new(Journal {
        dir: dir.to_path_buf(),
        opts,
        fs,
        mu: Mutex::new(State {
            next_lsn: 1,
            active: 0,
            active_size: 0,
            pending: Vec::new(),
            inflight: Vec::new(),
            appended: 0,
            written: 0,
            pending_len: 0,
            write_err: None,
            attempts: 0,
            closed: false,
        }),
        done: Condvar::new(),
        started: AtomicBool::new(false),
        flushed: AtomicU64::new(0),
        kick: Signal::new(),
        stop: Stop::new(),
        threads: Mutex::new(Vec::new()),
        holds: AtomicI32::new(0),
        sync_mu: Mutex::new(()),
        sync_kick: Signal::new(),
        unsynced: AtomicU64::new(0),
        files: RwLock::new(Files::default()),
    }))
}

fn new_chunk_cap(size: usize) -> usize {
    if size > CHUNK_BUFFER_SIZE {
        return size;
    }

    CHUNK_BUFFER_SIZE
}

fn copy_from_chunks(out: &mut [u8], loc: Location, chunks: &[Arc<Chunk>]) -> bool {
    for c in chunks {
        if c.segment != loc.segment
            || loc.offset < c.offset
            || loc.offset.wrapping_add(loc.length) > c.offset.wrapping_add(c.data.len() as u32)
        {
            continue;
        }

        let src = &c.data[(loc.offset - c.offset) as usize..];
        let n = out.len().min(src.len());
        out[..n].copy_from_slice(&src[..n]);

        return true;
    }

    false
}

pub struct ReadWindow {
    buf: Vec<u8>,
    segment: u32,
    start: u32,
    size: u32,
    pace: Option<Box<dyn FnMut(Duration) + Send>>,
}

pub fn new_read_window(size: usize, pace: Option<Box<dyn FnMut(Duration) + Send>>) -> ReadWindow {
    ReadWindow {
        buf: vec![0u8; size],
        segment: 0,
        start: 0,
        size: 0,
        pace,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SegmentInfo {
    pub id: u32,
    pub size: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayStats {
    pub set_aside: Vec<u32>,
    pub damaged: Vec<u32>,
    pub batches: usize,
    pub records: usize,
    pub truncated: i64,
    pub last_lsn: u64,
}

fn valid_batch_after(data: &[u8], mut from: usize, segment: u32) -> bool {
    let magic = BATCH_MAGIC.to_le_bytes();

    while from < data.len() {
        let Some(i) = data[from..].windows(magic.len()).position(|w| w == magic) else {
            return false;
        };

        from += i;
        if decode_batch(&data[from..], segment, from as u32).is_ok() {
            return true;
        }
        from += 1;
    }

    false
}

impl fmt::Debug for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ticket")
            .field("journal", &self.j.as_ref().map(|j| j.dir.clone()))
            .field("at", &self.at)
            .finish()
    }
}

impl Ticket {
    pub fn wait(&self) -> Result<(), BoxError> {
        match &self.j {
            None => Ok(()),
            Some(j) if j.opts.mode == Mode::Background => Ok(()),
            Some(j) => j.wait_written(self.at),
        }
    }
}

impl Journal {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn start(self: &Arc<Self>) {
        if self
            .started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let writer = self.clone();
            let syncer = self.clone();
            let mut threads = self.threads.lock();
            threads.push(spawn("journal-writer", move || writer.writer()));
            threads.push(spawn("journal-syncer", move || syncer.syncer()));
        }
    }

    pub fn set_next_lsn(&self, lsn: u64) {
        let mut st = self.mu.lock();
        if lsn > st.next_lsn {
            st.next_lsn = lsn;
        }
    }

    pub fn next_lsn(&self) -> u64 {
        self.mu.lock().next_lsn
    }

    pub fn position(&self) -> Position {
        let st = self.mu.lock();
        Journal::position_locked(&st)
    }

    fn position_locked(st: &State) -> Position {
        if st.active == 0 {
            return Position {
                segment: 1,
                offset: SEGMENT_HEADER_SIZE as u32,
            };
        }

        Position {
            segment: st.active,
            offset: st.active_size,
        }
    }

    pub fn append(self: &Arc<Self>, recs: &mut [Record]) -> Result<Ticket, Error> {
        if recs.is_empty() {
            return Ok(Ticket::default());
        }

        let size = batch_size(recs);
        if size as i64 > i64::from(u32::MAX) - SEGMENT_HEADER_SIZE as i64 {
            return Err(Error::TooLarge(size));
        }

        let mut st = self.mu.lock();
        if st.closed {
            return Err(Error::Closed);
        }

        if st.active == 0
            || (i64::from(st.active_size) + size as i64 > self.opts.segment_size
                && st.active_size as usize > SEGMENT_HEADER_SIZE)
        {
            Journal::rotate_locked(&mut st);
        }

        let (active, active_size, next_lsn) = (st.active, st.active_size, st.next_lsn);
        let idx = Journal::tail_chunk_locked(&mut st, size);
        let c = Arc::make_mut(&mut st.pending[idx]);
        let before = c.data.len();
        append_batch(&mut c.data, recs, next_lsn, active_size, active);
        let written = c.data.len() - before;
        st.next_lsn = st.next_lsn.wrapping_add(recs.len() as u64);
        st.active_size = st.active_size.wrapping_add(written as u32);
        st.appended += written as u64;
        st.pending_len += written;
        let t = Ticket {
            j: Some(self.clone()),
            at: st.appended,
        };
        let urgent = self.opts.mode != Mode::Background || st.pending_len >= self.opts.max_pending;
        drop(st);

        if urgent {
            self.signal();
        }

        Ok(t)
    }

    fn rotate_locked(st: &mut State) {
        st.active = st.active.wrapping_add(1);
        st.active_size = 0;
        let header = segment_header(st.active);
        Journal::append_bytes_locked(st, &header, true);
    }

    fn tail_chunk_locked(st: &mut State, size: usize) -> usize {
        if let Some(c) = st.pending.last()
            && c.segment == st.active
            && c.offset.wrapping_add(c.data.len() as u32) == st.active_size
            && c.cap.saturating_sub(c.data.len()) >= size
        {
            return st.pending.len() - 1;
        }

        st.pending.push(Arc::new(Chunk {
            segment: st.active,
            offset: st.active_size,
            data: Vec::with_capacity(size),
            cap: new_chunk_cap(size),
            fresh: false,
        }));

        st.pending.len() - 1
    }

    fn append_bytes_locked(st: &mut State, data: &[u8], fresh: bool) {
        let (active, active_size) = (st.active, st.active_size);
        match st.pending.last_mut() {
            Some(last)
                if !fresh
                    && last.segment == active
                    && last.offset.wrapping_add(last.data.len() as u32) == active_size =>
            {
                let c = Arc::make_mut(last);
                c.data.extend_from_slice(data);
                c.cap = c.cap.max(c.data.len());
            }
            _ => {
                let mut buf = Vec::with_capacity(data.len());
                buf.extend_from_slice(data);
                st.pending.push(Arc::new(Chunk {
                    segment: active,
                    offset: active_size,
                    fresh,
                    data: buf,
                    cap: new_chunk_cap(data.len()),
                }));
            }
        }

        st.active_size = st.active_size.wrapping_add(data.len() as u32);
        st.appended += data.len() as u64;
        st.pending_len += data.len();
    }

    fn signal(&self) {
        self.kick.kick();
    }

    fn wait_written(&self, at: u64) -> Result<(), BoxError> {
        let mut st = self.mu.lock();

        let start = st.attempts;
        while st.written < at && !(st.write_err.is_some() && st.attempts > start) && !st.closed {
            st = self.done.wait(st);
        }

        if st.written >= at {
            return Ok(());
        }

        if let Some(e) = &st.write_err {
            return Err(shared(e));
        }

        Err(Box::new(Error::Closed))
    }

    pub fn flush(&self) -> Result<(), BoxError> {
        let target = self.mu.lock().appended;

        self.signal();
        self.wait_written(target)?;

        self.sync_dirty()
    }

    fn sync_dirty(&self) -> Result<(), BoxError> {
        let _sync = self.sync_mu.lock();

        let (mut ids, files) = {
            let mut f = self.files.write();
            let mut ids = Vec::with_capacity(f.dirty.len());
            let mut files = HashMap::with_capacity(f.dirty.len());
            for &id in &f.dirty {
                if let Some(file) = f.files.get(&id) {
                    ids.push(id);
                    files.insert(id, file.clone());
                }
            }
            f.dirty = HashSet::new();
            (ids, files)
        };

        sort::slice(&mut ids, |a, b| a < b);

        let mut errs: Vec<Option<BoxError>> = Vec::new();
        for id in ids {
            if let Err(e) = files[&id].sync() {
                errs.push(Some(Box::new(e)));
                self.files.write().dirty.insert(id);
            }
        }

        match goerrors::join(errs) {
            Some(joined) => Err(Box::new(joined)),
            None => Ok(()),
        }
    }

    pub fn failure(&self) -> Option<BoxError> {
        self.mu.lock().write_err.as_ref().map(shared)
    }

    fn syncer(&self) {
        loop {
            self.sync_kick.wait();
            if self.stop.is_stopped() {
                return;
            }
            let _ = self.sync_dirty();
        }
    }

    fn note_written(&self, n: u64) {
        if self.opts.mode == Mode::Sync || self.opts.sync_bytes <= 0 {
            return;
        }

        let unsynced = self.unsynced.load(Ordering::Relaxed) + n;
        if unsynced < self.opts.sync_bytes as u64 {
            self.unsynced.store(unsynced, Ordering::Relaxed);
            return;
        }

        self.unsynced.store(0, Ordering::Relaxed);
        self.sync_kick.kick();
    }

    fn writer(&self) {
        let mut ticker =
            if self.opts.mode == Mode::Background && !self.opts.flush_interval.is_zero() {
                Some(Ticker::new(self.opts.flush_interval))
            } else {
                None
            };

        let mut backoff = Duration::ZERO;
        loop {
            match ticker.as_mut() {
                Some(t) => {
                    t.wait_signal(&self.kick);
                }
                None => self.kick.wait(),
            }

            if self.stop.is_stopped() {
                let _ = self.write_once();
                return;
            }

            if self.write_once().is_err() {
                backoff = (backoff * 2).clamp(Duration::from_millis(50), Duration::from_secs(2));
                if self.stop.wait_timeout(backoff) {
                    let _ = self.write_once();
                    return;
                }
                self.signal();
                continue;
            }

            backoff = Duration::ZERO;
            self.close_retired_all(false);
        }
    }

    fn write_once(&self) -> Result<(), BoxError> {
        let (chunks, target, pending_len) = {
            let mut st = self.mu.lock();
            if st.pending.is_empty() {
                return Ok(());
            }

            let chunks = std::mem::take(&mut st.pending);
            st.inflight = chunks.clone();
            let target = st.appended;
            let pending_len = st.pending_len;
            st.pending_len = 0;
            (chunks, target, pending_len)
        };

        let mut res = self.write_chunks(&chunks);
        if res.is_ok() && self.opts.mode == Mode::Sync {
            res = self.sync_dirty();
        }
        if res.is_ok() {
            self.note_written(pending_len as u64);
        }

        if let (Err(e), Some(hook)) = (&res, &self.opts.on_write_error) {
            hook(&**e);
        }

        let mut st = self.mu.lock();
        st.attempts += 1;
        match res {
            Err(e) => {
                st.inflight.clear();
                let mut pending = chunks;
                pending.append(&mut st.pending);
                st.pending = pending;
                st.pending_len += pending_len;
                let err: Arc<dyn StdError + Send + Sync> = Arc::from(e);
                st.write_err = Some(err.clone());
                self.done.notify_all();
                drop(st);

                Err(Box::new(SharedError(err)))
            }
            Ok(()) => {
                st.inflight.clear();
                st.written = target;
                st.write_err = None;
                if let Some(last) = chunks.last() {
                    self.flushed.store(
                        (u64::from(last.segment) << 32)
                            | u64::from(last.offset.wrapping_add(last.data.len() as u32)),
                        Ordering::SeqCst,
                    );
                }
                self.done.notify_all();

                Ok(())
            }
        }
    }

    fn write_chunks(&self, chunks: &[Arc<Chunk>]) -> Result<(), BoxError> {
        for c in chunks {
            let f = self.file_for(c.segment, c.fresh && c.offset == 0)?;

            let (n, err) = f.write_at(&c.data, u64::from(c.offset));
            if let Some(e) = err {
                return Err(Box::new(e));
            }

            if n != c.data.len() {
                return Err(Box::new(Error::ShortWrite {
                    segment: c.segment,
                    n,
                    len: c.data.len(),
                }));
            }

            self.files.write().dirty.insert(c.segment);
        }

        Ok(())
    }

    fn segment_path(&self, id: u32) -> PathBuf {
        filepath::join(&self.dir, segment_name(id))
    }

    fn file_for(&self, segment: u32, create: bool) -> Result<Arc<dyn File>, FsError> {
        {
            let files = self.files.read();
            let mut found = files.files.get(&segment).cloned();
            if found.is_none() && !create {
                found = files
                    .retired
                    .iter()
                    .find(|r| r.id == segment)
                    .map(|r| r.file.clone());
            }
            if let Some(f) = found {
                return Ok(f);
            }
        }

        let mut flags = OpenFlags::RDWR;
        if create {
            flags = flags | OpenFlags::CREATE;
        }

        let f = self
            .fs
            .open_file(&self.segment_path(segment), flags, 0o600)?;

        {
            let mut files = self.files.write();
            if let Some(existing) = files.files.get(&segment).cloned() {
                drop(files);
                let _ = f.close();
                return Ok(existing);
            }
            files.files.insert(segment, f.clone());
        }

        if create {
            self.fs.sync_dir(&self.dir)?;
            durable::preallocate(&*f, &self.dir, self.opts.segment_size);

            let previous = self
                .files
                .read()
                .files
                .get(&segment.wrapping_sub(1))
                .cloned();
            if let Some(previous) = previous {
                durable::trim_preallocated(&*previous);
            }
        }

        Ok(f)
    }

    pub fn read_value(&self, loc: Location) -> Result<Vec<u8>, BoxError> {
        let mut out = vec![0u8; loc.length as usize];
        if loc.length == 0 {
            return Ok(out);
        }

        let end = (u64::from(loc.segment) << 32) | u64::from(loc.offset.wrapping_add(loc.length));
        if end > self.flushed.load(Ordering::SeqCst) {
            let st = self.mu.lock();
            let found = copy_from_chunks(&mut out, loc, &st.pending)
                || copy_from_chunks(&mut out, loc, &st.inflight);
            drop(st);
            if found {
                return Ok(out);
            }
        }

        let f = self.file_for(loc.segment, false)?;

        let (n, err) = f.read_at(&mut out, u64::from(loc.offset));
        if n == out.len() {
            return Ok(out);
        }

        match err {
            Some(e) => Err(Box::new(e)),
            None => Err(Box::new(Error::ShortRead(loc.segment))),
        }
    }

    pub fn read_through<'w>(
        &self,
        loc: Location,
        w: &'w mut ReadWindow,
    ) -> Result<Cow<'w, [u8]>, BoxError> {
        let end = (u64::from(loc.segment) << 32) | u64::from(loc.offset.wrapping_add(loc.length));
        if loc.length == 0
            || loc.length as usize > w.buf.len()
            || end > self.flushed.load(Ordering::SeqCst)
        {
            return self.read_value(loc).map(Cow::Owned);
        }

        if w.size == 0
            || w.segment != loc.segment
            || loc.offset < w.start
            || loc.offset.wrapping_add(loc.length) > w.start.wrapping_add(w.size)
        {
            let f = self.file_for(loc.segment, false)?;

            let start = Instant::now();
            let (n, err) = f.read_at(&mut w.buf, u64::from(loc.offset));
            if let Some(pace) = w.pace.as_mut() {
                pace(start.elapsed());
            }
            if n < loc.length as usize {
                w.size = 0;
                return Err(match err {
                    Some(e) => Box::new(e),
                    None => Box::new(Error::ShortRead(loc.segment)),
                });
            }

            w.segment = loc.segment;
            w.start = loc.offset;
            w.size = n as u32;
        }

        let from = (loc.offset - w.start) as usize;
        Ok(Cow::Borrowed(&w.buf[from..from + loc.length as usize]))
    }

    pub fn bytes_from(&self, from: Position) -> i64 {
        let Ok(segments) = self.segments() else {
            return 0;
        };

        let mut n: i64 = 0;
        for seg in segments {
            if seg.id < from.segment {
                continue;
            }

            n += seg.size;
            if seg.id == from.segment {
                n -= i64::from(from.offset);
            }
        }

        n.max(0)
    }

    pub fn segments(&self) -> Result<Vec<SegmentInfo>, FsError> {
        let entries = match self.fs.read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.is_not_exist() => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };

        let mut out = Vec::new();
        for e in entries {
            let name = e.name().as_bytes();
            let Some(stem) = name.strip_suffix(SEGMENT_SUFFIX.as_bytes()) else {
                continue;
            };

            let Ok(id) = strconv::parse_uint(stem, 10, 32) else {
                continue;
            };

            let Some(size) = e.size() else {
                continue;
            };

            out.push(SegmentInfo {
                id: id as u32,
                size: size as i64,
            });
        }

        sort::slice(&mut out, |a, b| a.id < b.id);

        Ok(out)
    }

    pub fn active_segment(&self) -> u32 {
        self.mu.lock().active
    }

    pub fn replay<F>(&self, from: Position, mut apply: F) -> Result<ReplayStats, BoxError>
    where
        F: FnMut(&mut [Record]) -> Result<(), BoxError>,
    {
        let mut stats = ReplayStats::default();
        let segments = self.segments()?;

        let mut end = Position::default();
        for (idx, seg) in segments.iter().enumerate() {
            if seg.id < from.segment {
                continue;
            }

            let last = idx == segments.len() - 1;
            let mut start = SEGMENT_HEADER_SIZE as u32;
            if seg.id == from.segment && from.offset > start {
                start = from.offset;
            }

            let (stop, truncated, torn) =
                self.replay_segment(seg.id, start, &mut apply, &mut stats)?;

            stats.truncated += truncated;
            end = Position {
                segment: seg.id,
                offset: stop,
            };

            if torn && !last {
                for later in &segments[idx + 1..] {
                    stats.set_aside.push(later.id);
                    self.set_aside(later.id)?;
                }
                break;
            }
        }

        let mut st = self.mu.lock();
        if end.segment != 0 {
            st.active = end.segment;
            st.active_size = end.offset;
            st.written = 0;
            st.appended = 0;
            self.flushed.store(
                (u64::from(end.segment) << 32) | u64::from(end.offset),
                Ordering::SeqCst,
            );
        }
        if stats.last_lsn.wrapping_add(1) > st.next_lsn {
            st.next_lsn = stats.last_lsn.wrapping_add(1);
        }
        drop(st);

        Ok(stats)
    }

    fn keep_damaged_tail(&self, id: u32, tail: &[u8]) -> Result<(), FsError> {
        let path = filepath::join(
            &self.dir,
            format!("{}.tail{DAMAGED_SUFFIX}", segment_name(id)),
        );

        durable::write_file_atomic(&*self.fs, &path, |w| w.write(tail))
    }

    fn set_aside(&self, id: u32) -> Result<(), FsError> {
        let path = self.segment_path(id);
        let mut to = path.clone().into_os_string();
        to.push(DAMAGED_SUFFIX);

        self.fs.rename(&path, Path::new(&to))
    }

    fn replay_segment<F>(
        &self,
        id: u32,
        start: u32,
        apply: &mut F,
        stats: &mut ReplayStats,
    ) -> Result<(u32, i64, bool), BoxError>
    where
        F: FnMut(&mut [Record]) -> Result<(), BoxError>,
    {
        let path = self.segment_path(id);
        let f = self.fs.open_file(&path, OpenFlags::RDWR, 0o600)?;

        let size = match f.size() {
            Ok(size) => size,
            Err(e) => {
                let _ = f.close();
                return Err(Box::new(e));
            }
        };

        let mut data = vec![0u8; size as usize];
        let _ = f.read_at(&mut data, 0);

        if data.len() < SEGMENT_HEADER_SIZE || check_segment_header(&data, id).is_err() {
            if let Err(e) = f.truncate(0) {
                let _ = f.close();
                return Err(Box::new(e));
            }

            if let (_, Some(e)) = f.write_at(&segment_header(id), 0) {
                let _ = f.close();
                return Err(Box::new(e));
            }

            self.adopt_file(id, f);

            return Ok((SEGMENT_HEADER_SIZE as u32, data.len() as i64, true));
        }

        let mut pos = start as usize;
        while pos < data.len() {
            let (mut recs, n) = match decode_batch(&data[pos..], id, pos as u32) {
                Ok(decoded) => decoded,
                Err(_) => {
                    if valid_batch_after(&data, pos + 1, id) {
                        if let Err(e) = self.keep_damaged_tail(id, &data[pos..]) {
                            let _ = f.close();
                            return Err(Box::new(e));
                        }
                        stats.damaged.push(id);
                    }

                    let truncated = (data.len() - pos) as i64;
                    if let Err(e) = f.truncate(pos as u64) {
                        let _ = f.close();
                        return Err(Box::new(e));
                    }

                    self.adopt_file(id, f);

                    return Ok((pos as u32, truncated, true));
                }
            };

            for r in &recs {
                if r.lsn > stats.last_lsn {
                    stats.last_lsn = r.lsn;
                }
            }

            if let Err(e) = apply(&mut recs) {
                let _ = f.close();
                return Err(e);
            }

            stats.batches += 1;
            stats.records += recs.len();
            pos += n;
        }

        self.adopt_file(id, f);

        Ok((data.len() as u32, 0, false))
    }

    fn adopt_file(&self, id: u32, f: Arc<dyn File>) {
        let mut files = self.files.write();
        if let Some(existing) = files.files.remove(&id) {
            let _ = existing.close();
        }
        files.files.insert(id, f);
    }

    pub fn scan_segment<F>(&self, id: u32, mut func: F) -> Result<(), BoxError>
    where
        F: FnMut(&mut [Record]) -> Result<(), BoxError>,
    {
        let f = self.file_for(id, false)?;

        let size = f.size()?;

        let mut data = vec![0u8; size as usize];
        let _ = f.read_at(&mut data, 0);

        check_segment_header(&data, id)?;

        let mut pos = SEGMENT_HEADER_SIZE;
        while pos < data.len() {
            let (mut recs, n) = decode_batch(&data[pos..], id, pos as u32).map_err(|e| {
                goerrors::wrap(&format!("segment {id} offset {pos}: "), Box::new(e))
            })?;

            func(&mut recs)?;

            pos += n;
        }

        Ok(())
    }

    pub fn retire(&self, id: u32) -> Result<(), BoxError> {
        let active = self.mu.lock().active;
        if id >= active {
            return Err(Box::new(Error::StillActive(id)));
        }

        let path = self.segment_path(id);

        {
            let mut files = self.files.write();
            let mut f = files.files.get(&id).cloned();
            if f.is_none()
                && let Ok(opened) = self.fs.open_file(&path, OpenFlags::RDONLY, 0o600)
            {
                f = Some(opened);
            }
            files.files.remove(&id);
            files.dirty.remove(&id);
            if let Some(file) = f {
                files.retired.push(RetiredFile {
                    id,
                    file,
                    since: Instant::now(),
                });
            }
        }

        self.fs.remove(&path).map_err(|e| Box::new(e) as BoxError)
    }

    pub fn hold(&self) {
        self.holds.fetch_add(1, Ordering::SeqCst);
    }

    pub fn release(&self) {
        if self.holds.fetch_sub(1, Ordering::SeqCst) - 1 == 0 {
            self.close_retired_all(false);
        }
    }

    pub fn close_retired(&self) {
        self.close_retired_all(false);
    }

    fn close_retired_all(&self, all: bool) {
        if !all && self.holds.load(Ordering::SeqCst) > 0 {
            return;
        }

        let grace = self.opts.retire_grace;
        let mut files = self.files.write();
        files.retired.retain(|r| {
            if all || r.since.elapsed() > grace {
                let _ = r.file.close();
                return false;
            }

            true
        });
    }

    pub fn pending_bytes(&self) -> usize {
        self.mu.lock().pending_len
    }

    pub fn close(&self) -> Result<(), BoxError> {
        if self.mu.lock().closed {
            return Ok(());
        }

        self.stop_writer();

        let (mut err, active) = {
            let mut st = self.mu.lock();
            st.closed = true;
            let mut err = st.write_err.as_ref().map(shared);
            if !st.pending.is_empty() && err.is_none() {
                err = Some(Box::new(Error::UnwrittenAtClose) as BoxError);
            }
            self.done.notify_all();
            (err, st.active)
        };

        if let Err(sync_err) = self.sync_dirty()
            && err.is_none()
        {
            err = Some(sync_err);
        }

        {
            let mut files = self.files.write();
            if let Some(last) = files.files.get(&active) {
                durable::trim_preallocated(&**last);
            }
            for (_, f) in files.files.drain() {
                let _ = f.close();
            }
        }
        self.close_retired_all(true);

        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn stop_writer(&self) {
        if self.started.load(Ordering::SeqCst) {
            self.stop.stop();
            self.kick.kick();
            self.sync_kick.kick();
            let threads = std::mem::take(&mut *self.threads.lock());
            for t in threads {
                let _ = t.join();
            }
        }
    }

    pub fn abandon(&self) {
        {
            let mut st = self.mu.lock();
            if st.closed {
                return;
            }

            st.closed = true;
            st.pending.clear();
            st.pending_len = 0;
            self.done.notify_all();
        }

        self.stop_writer();

        {
            let mut files = self.files.write();
            for (_, f) in files.files.drain() {
                let _ = f.close();
            }
        }
        self.close_retired_all(true);
    }
}
