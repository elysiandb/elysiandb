use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;

use crate::durable::{self, Fs, PageStore, filepath, find_errno};
use crate::gocompat::errors::{self, BoxError};
use crate::journal::{self, Journal, Position, Record};
use crate::log::{self, Logger, Throttle};
use crate::pagetree::{Loader, Tree};
use crate::sync::{Condvar, Mutex, MutexGuard, RwLock, Ticker, WaitGroup, spawn};

use super::cache::ValueCache;
use super::checkpoint::{CheckpointState, CkptState};
use super::entity::{DocRef, Draft, Entity, EntityState, Index, Root, Slot, decode_slot_value};
use super::indexkey::{IndexSpec, RefPath, key_seq, seq_key};
use super::kv::{KvStore, ShardGuards};
use super::migrate::MigrationReport;
use super::release::value_cache_limit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Locked,
    Closed,
    Conflict,
    DiskFull,
    WriteFailed,
    Aborted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Locked => "the data folder is used by another ElysianDB process",
            Error::Closed => "datastore: closed",
            Error::Conflict => "datastore: write conflict",
            Error::DiskFull => "disk full",
            Error::WriteFailed => "storage write failed",
            Error::Aborted => "datastore: aborted",
        })
    }
}

impl std::error::Error for Error {}

pub const ERR_LOCKED: Error = Error::Locked;
pub const ERR_CLOSED: Error = Error::Closed;
pub const ERR_CONFLICT: Error = Error::Conflict;
pub const ERR_DISK_FULL: Error = Error::DiskFull;
pub const ERR_WRITE_FAILED: Error = Error::WriteFailed;

pub fn is_error(err: &(dyn std::error::Error + 'static), target: Error) -> bool {
    errors::is(err, &target)
}

#[derive(Debug)]
pub struct NotAnObject(pub Vec<u8>);

impl fmt::Display for NotAnObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "document {} is not a JSON object",
            String::from_utf8_lossy(&self.0)
        )
    }
}

impl std::error::Error for NotAnObject {}

pub const LOCK_FILE: &str = "elysiandb.lock";
pub const LOG_FOLDER: &str = "log";
pub const MANIFEST_FILE: &str = "MANIFEST";

pub(crate) const ENGINE_LOG: Logger = log::for_component("storage");
pub(crate) static WRITE_FAILURES: Throttle = Throttle::new(Duration::from_secs(60));

pub type KvExpiredHook = Arc<dyn Fn(usize) + Send + Sync>;
pub type FreeSpaceProbe = Arc<dyn Fn() -> Result<i64, BoxError> + Send + Sync>;

#[derive(Clone, Default)]
pub struct Options {
    pub folder: PathBuf,
    pub shards: i64,
    pub durability: journal::Mode,
    pub flush_interval: Duration,
    pub checkpoint_every: Duration,
    pub max_log_bytes: i64,
    pub cache_bytes: i64,
    pub page_cache_bytes: i64,
    pub page_segment_size: i64,
    pub dirty_flush_bytes: i64,
    pub index_workers: i64,
    pub background: bool,
    pub segment_size: i64,
    pub fs: Option<Arc<dyn Fs>>,
    pub legacy_recovery: bool,
    pub on_kv_expired: Option<KvExpiredHook>,
    pub on_write_error: Option<journal::WriteErrorHook>,
    pub min_free_bytes: i64,
    pub free_space: Option<FreeSpaceProbe>,
    pub retired_file_grace: Duration,
}

static ENGINE_INSTANCES: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Catalog {
    pub(crate) entities: BTreeMap<Vec<u8>, Arc<Entity>>,
    pub(crate) types: Vec<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct EngineCounters {
    pub(crate) checkpoints: AtomicI64,
    pub(crate) checkpoint_nanos: AtomicI64,
    pub(crate) compacted_segments: AtomicI64,
    pub(crate) compacted_bytes: AtomicI64,
    pub(crate) conflicts: AtomicI64,
    pub(crate) replayed_records: AtomicI64,
    pub(crate) live_pages: AtomicI64,
    pub(crate) refused_writes: AtomicI64,
}

#[derive(Default)]
struct BellState {
    stopped: bool,
    flush: bool,
    flushed: u64,
}

#[derive(Default)]
pub(crate) struct Bell {
    st: Mutex<BellState>,
    cv: Condvar,
}

impl Bell {
    pub(crate) fn stop(&self) {
        let mut g = self.st.lock();
        g.stopped = true;
        self.cv.notify_all();
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.st.lock().stopped
    }

    pub(crate) fn request_flush(&self) {
        let mut g = self.st.lock();
        if !g.flush {
            g.flush = true;
            self.cv.notify_all();
        }
    }

    pub(crate) fn signal_flushed(&self) {
        let mut g = self.st.lock();
        g.flushed = g.flushed.wrapping_add(1);
        self.cv.notify_all();
    }

    pub(crate) fn flushed(&self) -> u64 {
        self.st.lock().flushed
    }

    pub(crate) fn wait_until(&self, deadline: Instant) -> bool {
        let mut g = self.st.lock();
        while !g.stopped {
            let (ng, timed_out) = self.cv.wait_until(g, deadline);
            g = ng;
            if timed_out {
                break;
            }
        }
        g.stopped
    }

    pub(crate) fn wait_flush_or(&self, deadline: Instant) -> Wake {
        let mut g = self.st.lock();
        loop {
            if g.stopped {
                return Wake::Stop;
            }
            if g.flush {
                g.flush = false;
                return Wake::Flush;
            }
            let (ng, timed_out) = self.cv.wait_until(g, deadline);
            g = ng;
            if timed_out && !g.stopped && !g.flush {
                return Wake::Tick;
            }
        }
    }

    pub(crate) fn wait_flushed(&self, seen: u64, deadline: Instant) -> Wake {
        let mut g = self.st.lock();
        loop {
            if g.flushed != seen {
                return Wake::Flush;
            }
            if g.stopped {
                return Wake::Stop;
            }
            let (ng, timed_out) = self.cv.wait_until(g, deadline);
            g = ng;
            if timed_out && g.flushed == seen && !g.stopped {
                return Wake::Tick;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    Tick,
    Flush,
    Stop,
}

#[derive(Clone)]
pub struct StopToken(Arc<Engine>);

impl StopToken {
    pub fn is_stopped(&self) -> bool {
        self.0.bell.is_stopped()
    }

    pub fn wait_timeout(&self, d: Duration) -> bool {
        self.0.bell.wait_until(Instant::now() + d)
    }
}

pub(crate) const BUILD_QUEUE: usize = 1024;

#[derive(Default)]
pub(crate) struct BuildQueue {
    pub(crate) q: Mutex<VecDeque<Arc<Entity>>>,
    pub(crate) cv: Condvar,
}

pub struct Engine {
    me: Weak<Engine>,
    instance: u64,
    pub(crate) opts: Options,
    dir: PathBuf,
    lock: Mutex<Option<std::fs::File>>,
    j: OnceLock<Arc<Journal>>,
    pub(crate) inline: bool,
    pub(crate) cache: ValueCache,
    pub(crate) pages: Arc<PageStore>,
    pub(crate) loader: Arc<dyn Loader>,
    pub(crate) ckpt: Mutex<CkptState>,
    pub(crate) live: super::entity::LiveBytes,
    pub(crate) catalog: RwLock<Catalog>,
    pub(crate) types_mu: Mutex<()>,
    pub(crate) kv: KvStore,
    pub(crate) ck_state: ArcSwapOption<CheckpointState>,
    pub(crate) segment_live: ArcSwapOption<BTreeMap<u32, i64>>,
    pub(crate) dirty: AtomicI64,
    pub(crate) backlog_bytes: AtomicI64,
    pub(crate) bell: Bell,
    pub(crate) builds: BuildQueue,
    pub(crate) build_order: AtomicU64,
    pub(crate) upgrade_from: AtomicI64,
    pub(crate) last_reclaim: Mutex<Option<Instant>>,
    pub(crate) reclaim: AtomicI32,
    pub(crate) disk_low: AtomicBool,
    last_disk_check: Mutex<Option<Instant>>,
    pub(crate) pressure_sweeps: AtomicI64,
    threads: Mutex<Vec<JoinHandle<()>>>,
    close_mu: Mutex<()>,
    pub(crate) closed: AtomicBool,
    pub(crate) writers: WaitGroup,
    migrated: Option<MigrationReport>,
    pub(crate) stats: EngineCounters,
}

fn fs_err(op: &'static str, path: &Path, err: std::io::Error) -> BoxError {
    Box::new(durable::FsError::new(op, path, err))
}

pub fn open(mut opts: Options) -> Result<Arc<Engine>, BoxError> {
    if opts.shards <= 0 {
        opts.shards = 64;
    }

    if opts.index_workers <= 0 {
        opts.index_workers = 1;
    }

    durable::OS.mkdir_all(&opts.folder, 0o755)?;

    let lock = lock_folder(&opts.folder)?;
    let mut lock = Some(lock);

    let manifests = durable::open_manifests(opts.fs.clone(), &opts.folder);
    let migrated = match super::migrate::migrate_if_needed(&opts, &manifests) {
        Ok(report) => report,
        Err(err) => {
            unlock_file(&mut lock);
            return Err(err);
        }
    };

    let pages = match durable::open_page_store(
        opts.fs.clone(),
        &filepath::join(&opts.folder, durable::PAGES_FOLDER),
        opts.page_cache_bytes,
        opts.page_segment_size,
    ) {
        Ok(p) => p,
        Err(err) => {
            unlock_file(&mut lock);
            return Err(Box::new(err));
        }
    };

    let instance = ENGINE_INSTANCES.fetch_add(1, Ordering::SeqCst) + 1;
    let start = Instant::now();
    let shards = opts.shards;
    let cache_bytes = opts.cache_bytes;
    let folder = opts.folder.clone();
    let loader: Arc<dyn Loader> = pages.clone();

    let e = Arc::new_cyclic(|me| Engine {
        me: me.clone(),
        instance,
        dir: folder.clone(),
        lock: Mutex::new(lock),
        j: OnceLock::new(),
        inline: cache_bytes <= 0,
        cache: ValueCache::new(value_cache_limit(cache_bytes), 64),
        pages: pages.clone(),
        loader,
        ckpt: Mutex::new(CkptState::new(manifests)),
        live: Default::default(),
        catalog: RwLock::new(Catalog {
            entities: BTreeMap::new(),
            types: Vec::new(),
        }),
        types_mu: Mutex::new(()),
        kv: KvStore::new(shards),
        ck_state: ArcSwapOption::empty(),
        segment_live: ArcSwapOption::empty(),
        dirty: AtomicI64::new(0),
        backlog_bytes: AtomicI64::new(0),
        bell: Bell::default(),
        builds: BuildQueue::default(),
        build_order: AtomicU64::new(0),
        upgrade_from: AtomicI64::new(0),
        last_reclaim: Mutex::new(None),
        reclaim: AtomicI32::new(0),
        disk_low: AtomicBool::new(false),
        last_disk_check: Mutex::new(None),
        pressure_sweeps: AtomicI64::new(0),
        threads: Mutex::new(Vec::new()),
        close_mu: Mutex::new(()),
        closed: AtomicBool::new(false),
        writers: WaitGroup::new(),
        migrated,
        stats: EngineCounters::default(),
        opts,
    });

    if e.opts.background {
        let weak = Arc::downgrade(&e);
        pages.pace(move |synced| match weak.upgrade() {
            Some(e) => e.flush_pause(synced),
            None => Duration::ZERO,
        });
        pages.close_retired_after(e.opts.retired_file_grace);
    }

    if let Err(err) = e.load() {
        let _ = e.pages.close();
        e.unlock();
        return Err(err);
    }

    if e.upgrade_from.load(Ordering::SeqCst) != 0
        && let Err(err) = e.finish_upgrade()
    {
        e.journal().abandon();
        let _ = e.pages.close();
        e.unlock();
        return Err(err);
    }

    if e.opts.background {
        e.start_background();
    }

    ENGINE_LOG.debug(
        "engine opened",
        crate::kv!["folder" => folder.as_os_str().as_encoded_bytes(), "duration_ms" => log::since(start)],
    );

    Ok(e)
}

fn lock_folder(folder: &Path) -> Result<std::fs::File, BoxError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    let path = filepath::join(folder, LOCK_FILE);
    let f = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
    {
        Ok(f) => f,
        Err(err) => return Err(fs_err("open", &path, err)),
    };

    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        drop(f);
        return Err(Box::new(errors::Wrapped::new(
            format!("{} ({})", Error::Locked, folder.display()),
            Box::new(Error::Locked),
        )));
    }

    Ok(f)
}

fn unlock_file(lock: &mut Option<std::fs::File>) {
    use std::os::fd::AsRawFd;

    if let Some(f) = lock.take() {
        unsafe {
            libc::flock(f.as_raw_fd(), libc::LOCK_UN);
        }
        drop(f);
    }
}

pub(crate) const REPLAY_ANNOUNCE_BYTES: i64 = 64 << 10;
pub(crate) const REPLAY_BYTES_PER_SECOND: i64 = 32 << 20;
pub(crate) const REPLAY_NOTE_EVERY: Duration = Duration::from_secs(5);
pub(crate) const DISK_CHECK_EVERY: Duration = Duration::from_secs(1);
pub(crate) const DISK_RECOVER_GAIN: i64 = 4;
pub(crate) const SCAN_WINDOW_BYTES: usize = 4 << 20;
pub(crate) const LOG_SYNC_BYTES: i64 = 8 << 20;

pub(crate) fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "panic with a non-text value".to_string()
}

impl Engine {
    pub(crate) fn arc(&self) -> Arc<Engine> {
        match self.me.upgrade() {
            Some(e) => e,
            None => unreachable!(),
        }
    }

    pub(crate) fn journal(&self) -> &Arc<Journal> {
        match self.j.get() {
            Some(j) => j,
            None => unreachable!(),
        }
    }

    pub(crate) fn every(&self, period: Duration, mut f: impl FnMut()) {
        let mut ticker = Ticker::new(period);
        loop {
            if self.bell.wait_until(ticker.deadline()) {
                return;
            }
            if ticker.take(Instant::now()).is_some() {
                f();
            }
        }
    }

    pub(crate) fn new_tree(&self) -> Tree {
        Tree::new(Some(self.loader.clone()))
    }

    pub(crate) fn files(&self) -> Arc<dyn Fs> {
        durable::or(self.opts.fs.clone())
    }

    fn unlock(&self) {
        unlock_file(&mut self.lock.lock());
    }

    pub fn instance(&self) -> u64 {
        self.instance
    }

    pub fn folder(&self) -> &Path {
        &self.dir
    }

    pub fn replayed_records(&self) -> i64 {
        self.stats.replayed_records.load(Ordering::SeqCst)
    }

    pub fn migration(&self) -> Option<&MigrationReport> {
        self.migrated.as_ref()
    }

    fn journal_options(&self) -> journal::Options {
        let mut o = journal::Options {
            mode: self.opts.durability,
            flush_interval: self.opts.flush_interval,
            segment_size: self.opts.segment_size,
            retire_grace: self.opts.retired_file_grace,
            fs: self.opts.fs.clone(),
            on_write_error: self.opts.on_write_error.clone(),
            ..journal::Options::default()
        };
        if self.opts.background {
            o.sync_bytes = LOG_SYNC_BYTES;
        }

        o
    }

    fn load(&self) -> Result<(), BoxError> {
        let j = journal::open(
            &filepath::join(&self.dir, LOG_FOLDER),
            self.journal_options(),
        )?;
        let _ = self.j.set(j);

        let (m, previous) = self.choose_manifest()?;
        {
            let mut ck = self.ckpt.lock();
            ck.current = m.clone();
            ck.previous = previous;
        }

        let mut from = Position {
            segment: 1,
            offset: 0,
        };
        if let Some(m) = &m {
            if m.format == super::checkpoint::MANIFEST_FORMAT_PAGES {
                self.restore_pages(m)?;

                from = Position {
                    segment: m.segment,
                    offset: m.offset,
                };
                self.ck_state.store(Some(Arc::new(CheckpointState {
                    pos: from,
                    at: None,
                })));
                self.journal().set_next_lsn(m.lsn.wrapping_add(1));
            } else {
                let path = filepath::join(&self.dir, super::checkpoint::os_name(&m.checkpoint));
                let ck = match super::checkpoint::read_checkpoint(&path) {
                    Ok(ck) => ck,
                    Err(err) => {
                        return Err(errors::wrap(
                            &format!(
                                "cannot read the checkpoint {}: ",
                                String::from_utf8_lossy(&m.checkpoint)
                            ),
                            err,
                        ));
                    }
                };

                self.restore(&ck)?;

                from = ck.pos;
                self.upgrade_from.store(m.format, Ordering::SeqCst);
                self.ck_state.store(Some(Arc::new(CheckpointState {
                    pos: ck.pos,
                    at: None,
                })));
                self.journal().set_next_lsn(ck.last_lsn.wrapping_add(1));
            }
        }

        let began = Instant::now();
        let pending = self.journal().bytes_from(from);
        if pending >= REPLAY_ANNOUNCE_BYTES {
            ENGINE_LOG.info(
                "replaying the log",
                crate::kv!["bytes" => pending, "estimated_seconds" => pending / REPLAY_BYTES_PER_SECOND + 1],
            );
        }

        let mut last_note = began;
        let mut applied = 0usize;
        let stats = self.journal().replay(from, |recs| {
            self.apply_batch(recs, None, true, &mut [], None);
            if self.backlog_bytes.load(Ordering::SeqCst) >= self.backlog_limit() {
                self.settle_all();
            }

            applied += recs.len();
            if last_note.elapsed() >= REPLAY_NOTE_EVERY {
                last_note = Instant::now();
                ENGINE_LOG.info(
                    "log replay in progress",
                    crate::kv!["records" => applied, "elapsed_ms" => log::since(began)],
                );
            }

            Ok(())
        });
        let stats = match stats {
            Ok(s) => s,
            Err(err) => return Err(errors::wrap("cannot replay the log: ", err)),
        };

        self.settle_all();

        self.stats
            .replayed_records
            .store(stats.records as i64, Ordering::SeqCst);
        if stats.truncated > 0 {
            ENGINE_LOG.warn(
                "torn batch dropped at the end of the log",
                crate::kv!["bytes" => stats.truncated],
            );
        }

        if !stats.set_aside.is_empty() || !stats.damaged.is_empty() {
            ENGINE_LOG.error(
                "the log is damaged before its end, what follows the damage is set aside and not replayed",
                crate::kv![
                    "segments" => log::json(&stats.set_aside),
                    "damaged_tails" => log::json(&stats.damaged),
                    "suffix" => ".damaged",
                ],
            );
        }

        if stats.records > 0 {
            ENGINE_LOG.warn(
                "unclean shutdown detected, log replayed",
                crate::kv![
                    "batches" => stats.batches,
                    "records" => stats.records,
                    "duration_ms" => log::since(began),
                ],
            );
        }

        self.journal().start();

        Ok(())
    }

    fn start_background(&self) {
        let mut threads = self.threads.lock();

        let e = self.arc();
        threads.push(spawn("expiry", move || e.expiry_loop()));
        let e = self.arc();
        threads.push(spawn("maintenance", move || e.maintenance_loop()));
        for _ in 0..self.opts.index_workers {
            let e = self.arc();
            threads.push(spawn("index-build", move || e.build_worker()));
        }
        let e = self.arc();
        threads.push(spawn("dirty", move || e.dirty_loop()));
        let e = self.arc();
        threads.push(spawn("backlog", move || e.backlog_loop()));

        if self.inline {
            let e = self.arc();
            drop(spawn("warm-values", move || e.warm_values()));
        }
    }

    pub fn run_background<F>(&self, f: F)
    where
        F: FnOnce(StopToken) + Send + 'static,
    {
        if !self.opts.background {
            return;
        }

        let _g = self.close_mu.lock();

        if self.closed.load(Ordering::SeqCst) {
            return;
        }

        let token = StopToken(self.arc());
        self.threads
            .lock()
            .push(spawn("background", move || f(token)));
    }

    fn stop_and_join(&self) {
        self.bell.stop();
        {
            let _q = self.builds.q.lock();
            self.builds.cv.notify_all();
        }

        loop {
            let handles: Vec<JoinHandle<()>> = std::mem::take(&mut *self.threads.lock());
            if handles.is_empty() {
                break;
            }
            for h in handles {
                if let Err(payload) = h.join() {
                    std::panic::resume_unwind(payload);
                }
            }
        }
    }

    fn mark_closed(&self) -> bool {
        let _g = self.close_mu.lock();
        self.closed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn close(&self) -> Result<(), BoxError> {
        if !self.mark_closed() {
            return Ok(());
        }

        self.stop_and_join();
        self.writers.wait();
        self.settle_all();

        let mut errs: Vec<Option<BoxError>> = Vec::new();
        if let Err(err) = self.checkpoint(true) {
            errs.push(Some(err));
        }

        if let Err(err) = self.journal().close() {
            errs.push(Some(err));
        }

        if let Err(err) = self.pages.close() {
            errs.push(Some(Box::new(err)));
        }

        self.unlock();

        match errors::join(errs) {
            None => Ok(()),
            Some(j) => {
                if j.errors().len() == 1 {
                    let mut list = j.into_errors();
                    return Err(list.remove(0));
                }
                Err(Box::new(j))
            }
        }
    }

    pub fn abandon(&self) {
        if !self.mark_closed() {
            return;
        }

        self.stop_and_join();
        self.journal().abandon();
        let _ = self.pages.close();
        self.unlock();
    }

    pub(crate) fn check_disk(&self) {
        if self.opts.min_free_bytes <= 0 {
            return;
        }

        {
            let mut last = self.last_disk_check.lock();
            if let Some(t) = *last
                && t.elapsed() < DISK_CHECK_EVERY
            {
                return;
            }
            *last = Some(Instant::now());
        }

        let free = match &self.opts.free_space {
            Some(read) => read(),
            None => durable::free_bytes(&self.dir).map_err(|e| Box::new(e) as BoxError),
        };
        let Ok(free) = free else {
            return;
        };

        let threshold = self.opts.min_free_bytes;
        if free < threshold
            && self
                .disk_low
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            ENGINE_LOG.error(
                "disk almost full, writes are refused until space comes back",
                crate::kv!["free_bytes" => free, "min_free_bytes" => threshold],
            );
        } else if free >= threshold + threshold / DISK_RECOVER_GAIN
            && self
                .disk_low
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            ENGINE_LOG.info(
                "disk space is back, writes are accepted again",
                crate::kv!["free_bytes" => free, "min_free_bytes" => threshold],
            );
        }
    }

    pub fn write_refusal(&self) -> Option<Error> {
        if let Some(err) = self.journal().failure() {
            if find_errno(err.as_ref(), libc::ENOSPC) {
                return Some(Error::DiskFull);
            }
            return Some(Error::WriteFailed);
        }

        if self.disk_low.load(Ordering::SeqCst) {
            return Some(Error::DiskFull);
        }

        None
    }

    pub fn note_refused_write(&self) {
        self.stats.refused_writes.fetch_add(1, Ordering::SeqCst);
    }

    pub fn flush_log(&self) -> Result<(), BoxError> {
        self.journal().flush()
    }

    pub fn refused_writes(&self) -> i64 {
        self.stats.refused_writes.load(Ordering::SeqCst)
    }

    pub(crate) fn refuse(&self, err: Error) -> BoxError {
        self.stats.refused_writes.fetch_add(1, Ordering::SeqCst);
        Box::new(err)
    }

    pub(crate) fn guarded(&self, task: &str, f: impl FnOnce()) {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(f)) {
            let (suppressed, ok) = WRITE_FAILURES.allow(task);
            if ok {
                let text = panic_text(payload.as_ref());
                ENGINE_LOG.error(
                    "a background task failed, it will run again",
                    &log::with_suppressed(crate::kv!["task" => task, "err" => text], suppressed),
                );
            }
        }
    }

    pub(crate) fn entity(&self, name: &[u8], create: bool) -> Option<Arc<Entity>> {
        let found = self.catalog.read().entities.get(name).cloned();
        if found.is_some() || !create {
            return found;
        }

        let mut c = self.catalog.write();
        if let Some(ent) = c.entities.get(name) {
            return Some(ent.clone());
        }

        let ent = self.new_entity(name);
        c.entities.insert(name.to_vec(), ent.clone());
        Some(ent)
    }

    pub(crate) fn entity_or_create(&self, name: &[u8]) -> Arc<Entity> {
        match self.entity(name, true) {
            Some(ent) => ent,
            None => unreachable!(),
        }
    }

    pub(crate) fn all_entities(&self) -> Vec<Arc<Entity>> {
        self.catalog.read().entities.values().cloned().collect()
    }

    pub fn types(&self) -> Vec<Vec<u8>> {
        self.catalog.read().types.clone()
    }

    pub fn type_exists(&self, name: &[u8]) -> bool {
        self.catalog.read().types.iter().any(|t| t == name)
    }

    pub(crate) fn apply_batch(
        &self,
        recs: &[Record],
        docs: Option<&[DocRef]>,
        replay: bool,
        held: &mut [(&Arc<Entity>, &mut EntityState)],
        kv_held: Option<&mut ShardGuards<'_>>,
    ) {
        let mut names: Vec<&[u8]> = Vec::new();
        for rec in recs {
            match rec.kind {
                journal::KIND_DOC_PUT
                | journal::KIND_DOC_MOVE
                | journal::KIND_DOC_DELETE
                | journal::KIND_ENTITY_RESET => {
                    if !names.contains(&rec.entity.as_slice()) {
                        names.push(&rec.entity);
                    }
                }
                _ => {}
            }
        }

        let extra: Vec<Arc<Entity>> = names
            .iter()
            .filter(|n| !held.iter().any(|(ent, _)| ent.name.as_slice() == **n))
            .map(|n| self.entity_or_create(n))
            .collect();
        let mut extra_guards: Vec<MutexGuard<'_, EntityState>> =
            extra.iter().map(|ent| ent.apply.lock()).collect();

        let mut slots: Vec<(&Arc<Entity>, Option<&mut EntityState>)> = Vec::new();
        for (ent, st) in held.iter_mut() {
            slots.push((ent, Some(&mut **st)));
        }
        for (ent, g) in extra.iter().zip(extra_guards.iter_mut()) {
            slots.push((ent, Some(&mut **g)));
        }

        let mut drafts: Vec<Draft<'_>> = Vec::new();
        let background = self.opts.background;
        let bulk = recs.len() >= super::backlog::BULK_BATCH_RECORDS;

        let mut kv_held = kv_held;
        for (i, rec) in recs.iter().enumerate() {
            let doc = docs.and_then(|d| d[i].clone());

            match rec.kind {
                journal::KIND_DOC_PUT
                | journal::KIND_DOC_MOVE
                | journal::KIND_DOC_DELETE
                | journal::KIND_ENTITY_RESET => {
                    let pos = match drafts.iter().position(|d| d.ent.name == rec.entity) {
                        Some(p) => p,
                        None => {
                            let Some(slot) =
                                slots.iter_mut().find(|(ent, _)| ent.name == rec.entity)
                            else {
                                continue;
                            };
                            let ent = slot.0;
                            let Some(st) = slot.1.take() else {
                                continue;
                            };
                            let mut d = Draft::new(self, ent, st);
                            d.deferred = d.deferred || (background && (replay || bulk));
                            drafts.push(d);
                            drafts.len() - 1
                        }
                    };

                    let d = &mut drafts[pos];
                    match rec.kind {
                        journal::KIND_DOC_PUT | journal::KIND_DOC_MOVE => d.put(rec, doc, replay),
                        journal::KIND_DOC_DELETE => d.del(rec, replay),
                        _ => d.reset(rec),
                    }
                }
                journal::KIND_ENTITY_CREATE => {
                    let mut c = self.catalog.write();
                    add_type_locked(&mut c.types, &rec.entity);
                }
                journal::KIND_ENTITY_DROP => {
                    let mut c = self.catalog.write();
                    drop_type_locked(&mut c.types, &rec.entity);
                }
                journal::KIND_KV_PUT | journal::KIND_KV_MOVE => self.kv.apply_put(rec, replay),
                journal::KIND_KV_DELETE | journal::KIND_KV_EXPIRE => self.kv.apply_delete(&rec.key),
                journal::KIND_KV_RESET => match kv_held.as_deref_mut() {
                    Some(g) => self.kv.clear_locked(g),
                    None => {
                        let mut g = self.kv.lock_all();
                        self.kv.clear_locked(&mut g);
                    }
                },
                _ => {}
            }
        }

        for d in drafts {
            let released = d.publish();
            if !released.is_empty() {
                self.release_values(released);
            }
        }
    }

    pub fn snapshot(&self, name: &[u8]) -> Snapshot {
        match self.entity(name, false) {
            None => Snapshot::new(self.arc(), None, Arc::new(self.empty_root(0, 0)), name),
            Some(ent) => {
                let r = ent.root.load_full();
                Snapshot::new(self.arc(), Some(ent), r, name)
            }
        }
    }

    pub fn snapshot_all(&self) -> BTreeMap<Vec<u8>, Snapshot> {
        let ents = self.all_entities();
        let guards: Vec<MutexGuard<'_, EntityState>> =
            ents.iter().map(|ent| ent.apply.lock()).collect();

        let mut out = BTreeMap::new();
        for ent in &ents {
            out.insert(
                ent.name.clone(),
                Snapshot::new(
                    self.arc(),
                    Some(ent.clone()),
                    ent.root.load_full(),
                    &ent.name,
                ),
            );
        }

        for g in guards.into_iter().rev() {
            drop(g);
        }

        out
    }

    pub fn ensure_index(&self, name: &[u8], spec: &IndexSpec) -> bool {
        let key = spec.name();
        let ent = self.entity_or_create(name);
        if let Some(idx) = ent.root.load().indexes.get(&key)
            && !idx.dormant
        {
            return idx.ready;
        }

        let mut st = ent.apply.lock();
        let r = ent.root.load_full();
        if let Some(idx) = r.indexes.get(&key)
            && !idx.dormant
        {
            return idx.ready;
        }

        ent.touch(&key);
        let mut nr = (*r).clone();
        let mut indexes = (*r.indexes).clone();
        if r.slots.is_empty() {
            indexes.insert(key, Index::ready(spec.clone(), Tree::default()));
            nr.indexes = Arc::new(indexes);
            ent.root.store(Arc::new(nr));
            return true;
        }

        indexes.insert(key.clone(), Index::building(spec.clone()));
        nr.indexes = Arc::new(indexes);
        st.building.insert(
            key.clone(),
            super::entity::BuildState {
                order: self.build_order.fetch_add(1, Ordering::SeqCst) + 1,
                pass: None,
            },
        );
        ent.root.store(Arc::new(nr));
        let small = r.slots.len() <= super::build::SMALL_ENTITY_DOCS;
        drop(st);

        if !small && self.schedule(&ent) {
            return false;
        }

        self.build_entity(&ent, false);

        match ent.root.load().indexes.get(&key) {
            Some(idx) => idx.ready,
            None => false,
        }
    }

    pub fn drop_indexes(&self, name: &[u8]) {
        let Some(ent) = self.entity(name, false) else {
            return;
        };

        let mut st = ent.apply.lock();
        let mut r = (**ent.root.load()).clone();
        r.indexes = Arc::new(BTreeMap::new());
        r.lagging = false;
        st.building.clear();
        self.forget_all_backlog(&ent);
        ent.backlog.lagging.store(false, Ordering::SeqCst);
        ent.root.store(Arc::new(r));
    }

    pub fn index_names(&self, name: &[u8]) -> Option<Vec<Vec<u8>>> {
        let ent = self.entity(name, false)?;
        Some(ent.root.load().indexes.keys().cloned().collect())
    }

    pub fn index_backlog(&self) -> usize {
        let mut n = 0;
        for ent in self.all_entities() {
            for idx in ent.root.load().indexes.values() {
                if !idx.ready && !idx.dormant {
                    n += 1;
                }
            }
        }
        n
    }
}

pub(crate) fn add_type_locked(types: &mut Vec<Vec<u8>>, name: &[u8]) {
    if !types.iter().any(|t| t == name) {
        types.push(name.to_vec());
    }
}

pub(crate) fn drop_type_locked(types: &mut Vec<Vec<u8>>, name: &[u8]) {
    types.retain(|t| t != name);
}

#[doc(hidden)]
pub struct SnapInner {
    pub(crate) e: Arc<Engine>,
    pub(crate) ent: Option<Arc<Entity>>,
    pub(crate) r: Arc<Root>,
    pub(crate) name: Vec<u8>,
}

#[derive(Clone)]
pub struct Snapshot(Arc<SnapInner>);

impl std::ops::Deref for Snapshot {
    type Target = SnapInner;

    fn deref(&self) -> &SnapInner {
        &self.0
    }
}

impl Snapshot {
    pub(crate) fn new(
        e: Arc<Engine>,
        ent: Option<Arc<Entity>>,
        r: Arc<Root>,
        name: &[u8],
    ) -> Snapshot {
        Snapshot(Arc::new(SnapInner {
            e,
            ent,
            r,
            name: name.to_vec(),
        }))
    }

    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn len(&self) -> usize {
        self.r.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn instance(&self) -> u64 {
        self.e.instance
    }

    pub fn version(&self) -> u64 {
        self.r.version
    }

    pub fn generation(&self) -> u64 {
        self.r.gen_
    }

    pub fn next_seq(&self) -> u32 {
        self.r.next_seq
    }

    pub fn refs(&self) -> &[RefPath] {
        &self.r.refs
    }

    pub fn indexed(&self, seq: u32) -> Doc {
        Doc {
            s: self.clone(),
            slot: Slot {
                seq,
                ..Slot::default()
            },
            lazy: true,
        }
    }

    pub fn get(&self, id: &[u8]) -> Option<Doc> {
        let slot = self.r.slot_by_id(id)?;
        Some(Doc {
            s: self.clone(),
            slot,
            lazy: false,
        })
    }

    pub fn by_seq(&self, seq: u32) -> Option<Doc> {
        let slot = self.r.slot_by_seq(seq)?;
        Some(Doc {
            s: self.clone(),
            slot,
            lazy: false,
        })
    }

    pub fn at(&self, i: i64) -> Option<Doc> {
        let (k, v) = self.r.slots.at(i)?;
        Some(Doc {
            s: self.clone(),
            slot: decode_slot_value(key_seq(&k), &v),
            lazy: false,
        })
    }

    pub fn ascend(&self, offset: i64, mut f: impl FnMut(Doc) -> bool) {
        self.r.slots.ascend_at(offset, |k, v| {
            f(Doc {
                s: self.clone(),
                slot: decode_slot_value(key_seq(k), v),
                lazy: false,
            })
        });
    }

    pub fn scan_values(&self, f: impl FnMut(&Doc, Result<&[u8], BoxError>) -> bool) {
        self.scan_values_paced(None, f);
    }

    pub fn scan_values_paced(
        &self,
        pace: Option<Box<dyn FnMut(Duration) + Send>>,
        mut f: impl FnMut(&Doc, Result<&[u8], BoxError>) -> bool,
    ) {
        let mut window = journal::new_read_window(SCAN_WINDOW_BYTES, pace);
        self.r.slots.scan(|k, v| {
            let slot = decode_slot_value(key_seq(k), v);
            let lsn = slot.lsn;
            let loc = slot.loc;
            let d = Doc {
                s: self.clone(),
                slot,
                lazy: false,
            };
            if self.e.closed.load(Ordering::SeqCst) {
                f(&d, Err(Box::new(Error::Closed)));
                return false;
            }

            if let Some(b) = self.e.cache.peek(lsn) {
                return f(&d, Ok(&b));
            }

            match self.e.journal().read_through(loc, &mut window) {
                Ok(raw) => f(&d, Ok(&raw)),
                Err(err) => f(&d, Err(err)),
            }
        });
    }

    pub fn rank_of_seq(&self, seq: u32) -> usize {
        self.r.slots.rank(&seq_key(seq))
    }

    pub fn index(&self, spec: &IndexSpec) -> Option<IndexView> {
        let name = spec.name();
        let idx = self.r.indexes.get(&name)?;
        if !idx.ready || self.r.lagging {
            return None;
        }

        if let Some(ent) = &self.ent {
            ent.touch(&name);
        }

        Some(IndexView {
            idx: idx.clone(),
            prefix: Vec::new(),
            base: 0,
            end: 0,
            sub: false,
        })
    }

    pub fn has_index(&self, spec: &IndexSpec) -> bool {
        self.r.indexes.contains_key(&spec.name())
    }

    pub fn index_dormant(&self, spec: &IndexSpec) -> bool {
        self.r
            .indexes
            .get(&spec.name())
            .is_some_and(|idx| idx.dormant)
    }
}

#[derive(Clone)]
pub struct Doc {
    s: Snapshot,
    slot: Slot,
    lazy: bool,
}

impl Doc {
    fn resolved(&self) -> Cow<'_, Slot> {
        if !self.lazy {
            return Cow::Borrowed(&self.slot);
        }

        Cow::Owned(self.s.r.slot_by_seq(self.slot.seq).unwrap_or_default())
    }

    pub fn id(&self) -> Cow<'_, [u8]> {
        match self.resolved() {
            Cow::Borrowed(s) => Cow::Borrowed(s.id.as_slice()),
            Cow::Owned(s) => Cow::Owned(s.id),
        }
    }

    pub fn seq(&self) -> u32 {
        self.slot.seq
    }

    pub fn lsn(&self) -> u64 {
        self.resolved().lsn
    }

    pub fn bytes(&self) -> Result<Arc<[u8]>, BoxError> {
        self.s.e.slot_bytes(&self.resolved())
    }

    pub fn decode(&self) -> Result<crate::gocompat::json::Map, BoxError> {
        let sl = self.resolved();
        let b = self.s.e.slot_bytes(&sl)?;

        match super::entity::decode_map(&b) {
            Ok(Some(m)) => Ok(m),
            _ => Err(Box::new(NotAnObject(sl.id.clone()))),
        }
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.s
    }
}

#[derive(Clone)]
pub struct IndexView {
    pub(crate) idx: Arc<Index>,
    pub(crate) prefix: Vec<u8>,
    pub(crate) base: i64,
    pub(crate) end: i64,
    pub(crate) sub: bool,
}

impl IndexView {
    pub(crate) fn of(idx: Arc<Index>) -> IndexView {
        IndexView {
            idx,
            prefix: Vec::new(),
            base: 0,
            end: 0,
            sub: false,
        }
    }

    pub fn len(&self) -> i64 {
        if self.sub {
            return self.sub_len();
        }

        if self.bitmap() {
            return self.bitmap_len();
        }

        self.idx.tree.len() as i64
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn rank(&self, prefix: &[u8]) -> i64 {
        if self.sub {
            return self.sub_rank(prefix);
        }

        if self.bitmap() {
            return self.bitmap_rank(prefix);
        }

        self.idx.tree.rank(prefix) as i64
    }

    pub fn scan_prefix(&self, prefix: &[u8], mut f: impl FnMut(&[u8], u32) -> bool) {
        if self.bitmap() {
            let end = prefix_end(prefix);
            self.bitmap_scan(Some(prefix), end.as_deref(), f);
            return;
        }

        let n = prefix.len();
        self.idx.tree.ascend_from(prefix, |k, _| {
            if k.len() < n + 4 || &k[..n] != prefix {
                return false;
            }

            f(k, key_seq(k))
        });
    }

    pub fn scan_range(
        &self,
        lo: Option<&[u8]>,
        hi: Option<&[u8]>,
        mut f: impl FnMut(&[u8], u32) -> bool,
    ) {
        if self.bitmap() {
            self.bitmap_scan(lo, hi, f);
            return;
        }

        self.idx.tree.ascend_from(lo.unwrap_or_default(), |k, _| {
            if let Some(hi) = hi
                && k >= hi
            {
                return false;
            }

            f(k, key_seq(k))
        });
    }

    pub fn ascend_at(&self, i: i64, mut f: impl FnMut(&[u8], u32) -> bool) {
        if self.sub {
            self.sub_ascend_at(i, f);
            return;
        }

        self.idx.tree.ascend_at(i, |k, _| f(k, key_seq(k)));
    }

    pub fn at(&self, i: i64) -> Option<(Vec<u8>, u32)> {
        if self.sub {
            return self.sub_at(i);
        }

        let (k, _) = self.idx.tree.at(i)?;
        let seq = key_seq(&k);
        Some((k.to_vec(), seq))
    }
}

pub fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    for i in (0..end.len()).rev() {
        if end[i] < 0xff {
            end[i] += 1;
            end.truncate(i + 1);
            return Some(end);
        }
    }

    None
}
