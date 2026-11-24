use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use super::disk::{preallocate, trim_preallocated};
use super::fs::{File, Fs, FsError, OpenFlags, filepath, or};
use super::kvfile::Error;
use crate::gocompat::errors::BoxError;
use crate::gocompat::strconv;
use crate::hash::crc32c;
use crate::pagetree::{self, Loader, Page, Ref, Tree};
use crate::sync::{Mutex, RwLock, spawn};

pub const PAGES_FOLDER: &str = "pages";
const PAGE_SEGMENT_SUFFIX: &str = ".pgs";
const PAGE_SEGMENT_SIZE: i64 = 64 << 20;
pub const PAGE_HEADER_SIZE: i64 = 8;
const PAGE_CLOSE_DELAY: Duration = Duration::from_secs(5 * 60);
const PAGE_FLUSH_BYTES: usize = 4 << 20;

type PaceFn = Box<dyn Fn(Duration) -> Duration + Send + Sync>;

struct CachedPage {
    r: Weak<Ref>,
    bytes: i64,
}

struct WriteState {
    cur: Option<Arc<dyn File>>,
    cur_id: u32,
    cur_off: i64,
    buf: Vec<u8>,
    buf_start: i64,
    flushing: Option<Arc<Vec<u8>>>,
    flush_start: i64,
    spare: Option<Vec<u8>>,
}

struct IoState {
    unsynced: BTreeMap<u32, Arc<dyn File>>,
    pace: Option<PaceFn>,
}

#[derive(Default)]
struct Files {
    files: BTreeMap<u32, Arc<dyn File>>,
    sizes: BTreeMap<u32, i64>,
    retired: BTreeMap<u32, Arc<dyn File>>,
}

struct Shared {
    files: RwLock<Files>,
    holds: AtomicI32,
    close_delay: AtomicU64,
}

struct Ring {
    ring: Vec<CachedPage>,
    hand: usize,
}

pub struct PageStore {
    fs: Arc<dyn Fs>,
    dir: PathBuf,
    seg_size: i64,
    write: Mutex<WriteState>,
    io: Mutex<IoState>,
    shared: Arc<Shared>,
    cache: Mutex<Ring>,
    resident: AtomicI64,
    limit: AtomicI64,
    loads: AtomicI64,
    writes: AtomicI64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    pub resident: i64,
    pub limit: i64,
    pub loads: i64,
    pub writes: i64,
}

fn page_segment_name(id: u32) -> String {
    format!("{id:06}{PAGE_SEGMENT_SUFFIX}")
}

fn page_id(seg: u32, off: i64) -> u64 {
    (u64::from(seg) << 32) | off as u64
}

pub fn split_page_id(id: u64) -> (u32, i64) {
    ((id >> 32) as u32, i64::from(id as u32))
}

fn duration_nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

pub fn open_page_store(
    fs: Option<Arc<dyn Fs>>,
    dir: &Path,
    limit: i64,
    seg_size: i64,
) -> Result<Arc<PageStore>, FsError> {
    let fs = or(fs);
    fs.mkdir_all(dir, 0o755)?;

    let seg_size = if seg_size <= 0 {
        PAGE_SEGMENT_SIZE
    } else {
        seg_size
    };

    let s = PageStore {
        fs,
        dir: dir.to_path_buf(),
        seg_size,
        write: Mutex::new(WriteState {
            cur: None,
            cur_id: 0,
            cur_off: 0,
            buf: Vec::new(),
            buf_start: 0,
            flushing: None,
            flush_start: 0,
            spare: None,
        }),
        io: Mutex::new(IoState {
            unsynced: BTreeMap::new(),
            pace: None,
        }),
        shared: Arc::new(Shared {
            files: RwLock::new(Files::default()),
            holds: AtomicI32::new(0),
            close_delay: AtomicU64::new(duration_nanos(PAGE_CLOSE_DELAY)),
        }),
        cache: Mutex::new(Ring {
            ring: Vec::new(),
            hand: 0,
        }),
        resident: AtomicI64::new(0),
        limit: AtomicI64::new(0),
        loads: AtomicI64::new(0),
        writes: AtomicI64::new(0),
    };
    s.limit.store(limit, Ordering::SeqCst);

    let entries = s.fs.read_dir(dir)?;

    for e in entries {
        let name = e.name().as_bytes();
        let Some(stem) = name.strip_suffix(PAGE_SEGMENT_SUFFIX.as_bytes()) else {
            continue;
        };

        let Ok(id) = strconv::parse_uint(stem, 10, 32) else {
            continue;
        };
        let id = id as u32;

        let f = match s
            .fs
            .open_file(&filepath::join(dir, e.name()), OpenFlags::RDWR, 0o600)
        {
            Ok(f) => f,
            Err(err) => {
                let _ = s.close();
                return Err(err);
            }
        };

        let size = match f.size() {
            Ok(size) => size,
            Err(err) => {
                let _ = f.close();
                let _ = s.close();
                return Err(err);
            }
        };

        {
            let mut files = s.shared.files.write();
            files.files.insert(id, f);
            files.sizes.insert(id, size as i64);
        }
        let mut w = s.write.lock();
        w.cur_id = w.cur_id.max(id);
    }

    Ok(Arc::new(s))
}

impl PageStore {
    pub fn close_retired_after(&self, d: Duration) {
        if d > Duration::ZERO {
            self.shared
                .close_delay
                .store(duration_nanos(d), Ordering::SeqCst);
        }
    }

    pub fn pace<F>(&self, f: F)
    where
        F: Fn(Duration) -> Duration + Send + Sync + 'static,
    {
        let mut io = self.io.lock();
        io.pace = Some(Box::new(f));
    }

    pub fn set_limit(&self, limit: i64) {
        self.limit.store(limit, Ordering::SeqCst);
        self.evict();
    }

    pub fn hold(&self) {
        self.shared.holds.fetch_add(1, Ordering::SeqCst);
    }

    pub fn release(&self) {
        self.shared.holds.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn peek(&self, r: &Arc<Ref>) -> Result<Arc<Page>, Error> {
        if let Some(p) = r.resident() {
            return Ok(p);
        }

        let b = self.read(r.id(), r.size())?;

        match pagetree::decode(&b) {
            Ok(p) => Ok(Arc::new(p)),
            Err(err) => Err(Error::Page(r.id(), Box::new(Error::CorruptPage(err)))),
        }
    }

    pub fn load(&self, r: &Arc<Ref>) -> Result<Arc<Page>, Error> {
        if let Some(p) = r.resident() {
            return Ok(p);
        }

        let b = self.read(r.id(), r.size())?;

        let p = match pagetree::decode(&b) {
            Ok(p) => Arc::new(p),
            Err(err) => return Err(Error::Page(r.id(), Box::new(Error::CorruptPage(err)))),
        };

        self.loads.fetch_add(1, Ordering::SeqCst);
        let got = r.attach(p.clone());
        if Arc::ptr_eq(&got, &p) {
            self.remember(r, &p);
        }

        Ok(got)
    }

    fn read(&self, id: u64, size: u32) -> Result<Vec<u8>, Error> {
        let (seg, off) = split_page_id(id);
        let need = PAGE_HEADER_SIZE + i64::from(size);

        {
            let w = self.write.lock();
            if seg == w.cur_id && w.cur.is_some() {
                if off >= w.buf_start {
                    let start = off - w.buf_start;
                    if start + need <= w.buf.len() as i64 {
                        let from = (start + PAGE_HEADER_SIZE) as usize;
                        return Ok(w.buf[from..from + size as usize].to_vec());
                    }
                } else if let Some(flushing) = &w.flushing
                    && off >= w.flush_start
                {
                    let start = off - w.flush_start;
                    if start + need <= flushing.len() as i64 {
                        let from = (start + PAGE_HEADER_SIZE) as usize;
                        return Ok(flushing[from..from + size as usize].to_vec());
                    }
                }
            }
        }

        let f = {
            let files = self.shared.files.read();
            files
                .files
                .get(&seg)
                .or_else(|| files.retired.get(&seg))
                .cloned()
        };
        let Some(f) = f else {
            return Err(Error::PageSegmentMissing(seg));
        };

        let mut b = vec![0u8; need as usize];
        let (_, err) = f.read_at(&mut b, off as u64);
        if let Some(err) = err {
            return Err(Error::Page(id, Box::new(Error::Fs(err))));
        }

        let stored = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let sum = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        if stored != size || crc32c::checksum(&b[PAGE_HEADER_SIZE as usize..]) != sum {
            return Err(Error::Page(id, Box::new(Error::PageChecksum)));
        }

        b.drain(..PAGE_HEADER_SIZE as usize);
        Ok(b)
    }

    fn remember(&self, r: &Arc<Ref>, p: &Arc<Page>) {
        let size = p.bytes();

        self.cache.lock().ring.push(CachedPage {
            r: Arc::downgrade(r),
            bytes: size,
        });

        if self.resident.fetch_add(size, Ordering::SeqCst) + size
            > self.limit.load(Ordering::SeqCst)
            && self.limit.load(Ordering::SeqCst) > 0
        {
            self.evict();
        }
    }

    fn evict(&self) {
        let limit = self.limit.load(Ordering::SeqCst);
        if limit <= 0 {
            return;
        }

        let mut c = self.cache.lock();

        let mut spins = 0usize;
        while self.resident.load(Ordering::SeqCst) > limit
            && !c.ring.is_empty()
            && spins < 2 * c.ring.len() + 2
        {
            spins += 1;

            if c.hand >= c.ring.len() {
                c.hand = 0;
            }

            let hand = c.hand;
            let r = c.ring[hand].r.upgrade();
            if let Some(r) = &r
                && r.take_hot()
            {
                c.hand += 1;
                continue;
            }

            if let Some(r) = &r {
                r.evict();
            }

            self.resident
                .fetch_sub(c.ring[hand].bytes, Ordering::SeqCst);
            c.ring.swap_remove(hand);
        }
    }

    pub fn sweep(&self) {
        let mut c = self.cache.lock();

        let resident = &self.resident;
        c.ring.retain(|e| {
            if let Some(r) = e.r.upgrade()
                && r.resident().is_some()
            {
                return true;
            }
            resident.fetch_sub(e.bytes, Ordering::SeqCst);
            false
        });

        c.hand = 0;
    }

    pub fn write(&self, r: &Arc<Ref>, p: &Arc<Page>) -> Result<(), BoxError> {
        let payload = pagetree::encode(Vec::new(), p)?;

        let mut io = self.io.lock();

        let rotate = {
            let w = self.write.lock();
            w.cur.is_none() || w.cur_off + payload.len() as i64 + PAGE_HEADER_SIZE > self.seg_size
        };

        if rotate {
            self.rotate_locked(&mut io)?;
        }

        let mut header = [0u8; PAGE_HEADER_SIZE as usize];
        header[..4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        header[4..].copy_from_slice(&crc32c::checksum(&payload).to_le_bytes());

        let (id, full) = {
            let mut w = self.write.lock();
            let off = w.cur_off;
            w.buf.extend_from_slice(&header);
            w.buf.extend_from_slice(&payload);
            w.cur_off += payload.len() as i64 + PAGE_HEADER_SIZE;
            (page_id(w.cur_id, off), w.buf.len() >= PAGE_FLUSH_BYTES)
        };

        if full {
            self.flush_locked(&mut io)?;
            self.settle(&mut io);
        }

        r.persisted(id, payload.len() as u32);
        self.writes.fetch_add(1, Ordering::SeqCst);

        let c = Arc::new(p.compact());
        let p = if r.replace(p, c.clone()) {
            c
        } else {
            p.clone()
        };
        self.remember(r, &p);

        Ok(())
    }

    fn settle(&self, io: &mut IoState) {
        let IoState { unsynced, pace } = io;
        let Some(pace) = pace else {
            return;
        };

        let (id, f) = {
            let w = self.write.lock();
            (w.cur_id, w.cur.clone())
        };
        let Some(f) = f else {
            return;
        };

        let start = Instant::now();
        if f.sync().is_err() {
            return;
        }
        let took = start.elapsed();
        unsynced.remove(&id);

        let pause = pace(took);
        if pause > Duration::ZERO {
            std::thread::sleep(pause);
        }
    }

    fn rotate_locked(&self, io: &mut IoState) -> Result<(), FsError> {
        self.flush_locked(io)?;

        let (id, previous) = {
            let w = self.write.lock();
            (w.cur_id.wrapping_add(1), w.cur.clone())
        };

        let path = filepath::join(&self.dir, page_segment_name(id));
        let f = self.fs.open_file(
            &path,
            OpenFlags::CREATE | OpenFlags::EXCL | OpenFlags::RDWR,
            0o600,
        )?;

        if let Err(err) = self.fs.sync_dir(&self.dir) {
            let _ = f.close();
            return Err(err);
        }

        if let Some(previous) = &previous {
            trim_preallocated(&**previous);
        }
        preallocate(&*f, &self.dir, self.seg_size);

        {
            let mut files = self.shared.files.write();
            files.files.insert(id, f.clone());
            files.sizes.insert(id, 0);
        }

        {
            let mut w = self.write.lock();
            w.cur = Some(f.clone());
            w.cur_id = id;
            w.cur_off = 0;
            w.buf_start = 0;
        }
        io.unsynced.insert(id, f);

        Ok(())
    }

    fn flush_locked(&self, io: &mut IoState) -> Result<(), FsError> {
        let (data, start, seg, f) = {
            let mut w = self.write.lock();
            let Some(f) = w.cur.clone() else {
                return Ok(());
            };
            if w.buf.is_empty() {
                return Ok(());
            }

            let next = w.spare.take().unwrap_or_default();
            let data = Arc::new(std::mem::replace(&mut w.buf, next));
            let start = w.buf_start;
            w.flushing = Some(data.clone());
            w.flush_start = start;
            w.buf_start = start + data.len() as i64;
            (data, start, w.cur_id, f)
        };

        let (_, err) = f.write_at(&data, start as u64);

        let len = data.len() as i64;
        {
            let mut w = self.write.lock();
            w.flushing = None;
            let mut data = Arc::try_unwrap(data).unwrap_or_else(|shared| (*shared).clone());
            if let Some(err) = err {
                data.extend_from_slice(&w.buf);
                w.buf = data;
                w.buf_start = start;
                return Err(err);
            }
            data.clear();
            w.spare = Some(data);
        }

        io.unsynced.insert(seg, f);

        self.shared.files.write().sizes.insert(seg, start + len);

        Ok(())
    }

    pub fn rotate(&self) -> Result<(), FsError> {
        let mut io = self.io.lock();

        let empty = {
            let w = self.write.lock();
            w.cur.is_none() || w.cur_off == 0
        };
        if empty {
            return Ok(());
        }

        self.rotate_locked(&mut io)
    }

    pub fn flush_tree(&self, t: &Tree) -> Result<(), BoxError> {
        t.for_each_dirty(|r, p| self.write(r, p))
    }

    pub fn sync(&self) -> Result<(), FsError> {
        let mut io = self.io.lock();

        self.flush_locked(&mut io)?;

        let ids: Vec<u32> = io.unsynced.keys().copied().collect();
        for id in ids {
            let Some(f) = io.unsynced.get(&id).cloned() else {
                continue;
            };
            f.sync()?;
            io.unsynced.remove(&id);
        }

        Ok(())
    }

    pub fn current_segment(&self) -> u32 {
        self.write.lock().cur_id
    }

    pub fn segment_sizes(&self) -> BTreeMap<u32, i64> {
        self.shared.files.read().sizes.clone()
    }

    pub fn remove(&self, ids: &mut [u32]) {
        if ids.is_empty() {
            return;
        }

        ids.sort_unstable();

        let mut retired = Vec::new();
        {
            let mut files = self.shared.files.write();
            for &id in ids.iter() {
                if let Some(f) = files.files.remove(&id) {
                    files.retired.insert(id, f);
                    retired.push(id);
                }
                files.sizes.remove(&id);
                let _ = self
                    .fs
                    .remove(&filepath::join(&self.dir, page_segment_name(id)));
            }
        }

        let _ = self.fs.sync_dir(&self.dir);

        self.close_later(retired);
    }

    fn close_later(&self, ids: Vec<u32>) {
        if ids.is_empty() {
            return;
        }

        let shared = self.shared.clone();
        spawn("pages-close", move || {
            loop {
                let delay = Duration::from_nanos(shared.close_delay.load(Ordering::SeqCst));
                std::thread::sleep(delay);
                if shared.holds.load(Ordering::SeqCst) > 0 {
                    continue;
                }

                let mut files = shared.files.write();
                for id in &ids {
                    if let Some(f) = files.retired.remove(id) {
                        let _ = f.close();
                    }
                }
                return;
            }
        });
    }

    pub fn close(&self) -> Result<(), FsError> {
        let err = {
            let mut io = self.io.lock();
            let mut err = self.flush_locked(&mut io).err();
            for (_, f) in std::mem::take(&mut io.unsynced) {
                if let Err(serr) = f.sync()
                    && err.is_none()
                {
                    err = Some(serr);
                }
            }

            let mut w = self.write.lock();
            if let Some(cur) = &w.cur {
                trim_preallocated(&**cur);
            }
            w.cur = None;
            err
        };

        {
            let mut files = self.shared.files.write();
            for (_, f) in std::mem::take(&mut files.files) {
                let _ = f.close();
            }
            for (_, f) in std::mem::take(&mut files.retired) {
                let _ = f.close();
            }
        }

        match err {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    pub fn stats(&self) -> PageStats {
        PageStats {
            resident: self.resident.load(Ordering::SeqCst),
            limit: self.limit.load(Ordering::SeqCst),
            loads: self.loads.load(Ordering::SeqCst),
            writes: self.writes.load(Ordering::SeqCst),
        }
    }
}

impl Loader for PageStore {
    fn load(&self, r: &Arc<Ref>) -> Result<Arc<Page>, BoxError> {
        PageStore::load(self, r).map_err(|e| Box::new(e) as BoxError)
    }

    fn peek(&self, r: &Arc<Ref>) -> Option<Result<Arc<Page>, BoxError>> {
        Some(PageStore::peek(self, r).map_err(|e| Box::new(e) as BoxError))
    }
}
