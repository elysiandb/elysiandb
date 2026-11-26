#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use elysiandb::durable::{self, DirEntry, File, Fs, FsError, OpenFlags, PageStore};
use elysiandb::pagetree::{Loader, Token, Tree};
use elysiandb::sync::{Mutex, Stop, spawn};

use super::memfs_test::{MemFs, new_mem_fs};
use super::tempdir::TempDir;

pub struct GateState {
    mem: MemFs,
    gate: Mutex<Option<Arc<Stop>>>,
    delay: Mutex<Duration>,
    pub syncs: AtomicI64,
}

#[derive(Clone)]
pub struct GateFs(pub Arc<GateState>);

impl GateFs {
    pub fn new(mem: MemFs) -> GateFs {
        GateFs(Arc::new(GateState {
            mem,
            gate: Mutex::new(None),
            delay: Mutex::new(Duration::ZERO),
            syncs: AtomicI64::new(0),
        }))
    }

    pub fn mem(&self) -> &MemFs {
        &self.0.mem
    }

    pub fn set_gate(&self, gate: Option<Arc<Stop>>) {
        *self.0.gate.lock() = gate;
    }

    pub fn set_delay(&self, d: Duration) {
        *self.0.delay.lock() = d;
    }

    pub fn syncs(&self) -> i64 {
        self.0.syncs.load(Ordering::SeqCst)
    }
}

impl Fs for GateFs {
    fn open_file(
        &self,
        path: &Path,
        flags: OpenFlags,
        mode: u32,
    ) -> Result<Arc<dyn File>, FsError> {
        let file = self.0.mem.open_file(path, flags, mode)?;

        Ok(Arc::new(GateFile {
            file,
            fs: self.clone(),
        }))
    }

    fn remove(&self, path: &Path) -> Result<(), FsError> {
        self.0.mem.remove(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        self.0.mem.rename(from, to)
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FsError> {
        self.0.mem.read_dir(path)
    }

    fn mkdir_all(&self, path: &Path, mode: u32) -> Result<(), FsError> {
        self.0.mem.mkdir_all(path, mode)
    }

    fn sync_dir(&self, path: &Path) -> Result<(), FsError> {
        self.0.mem.sync_dir(path)
    }
}

pub struct GateFile {
    file: Arc<dyn File>,
    fs: GateFs,
}

impl File for GateFile {
    fn read_at(&self, buf: &mut [u8], off: u64) -> (usize, Option<FsError>) {
        self.file.read_at(buf, off)
    }

    fn write_at(&self, buf: &[u8], off: u64) -> (usize, Option<FsError>) {
        self.file.write_at(buf, off)
    }

    fn sync(&self) -> Result<(), FsError> {
        self.fs.0.syncs.fetch_add(1, Ordering::SeqCst);
        let gate = self.fs.0.gate.lock().clone();
        if let Some(gate) = gate {
            gate.wait();
        }
        let delay = *self.fs.0.delay.lock();
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }

        self.file.sync()
    }

    fn truncate(&self, size: u64) -> Result<(), FsError> {
        self.file.truncate(size)
    }

    fn close(&self) -> Result<(), FsError> {
        self.file.close()
    }

    fn size(&self) -> Result<u64, FsError> {
        self.file.size()
    }
}

fn fill_tree(store: &Arc<PageStore>, n: usize, padding: usize) -> Tree {
    let tok = Token::new();
    let ld: Arc<dyn Loader> = store.clone();
    let mut tree = Tree::new(Some(ld)).with_token(Some(&tok));
    let value = vec![0u8; padding];
    for i in 0..n {
        tree = tree.put(format!("key-{i:08}").as_bytes(), &value).0;
    }
    tok.freeze();

    tree.with_token(None)
}

#[test]
fn page_reads_do_not_wait_for_a_sync() {
    let fs = GateFs::new(new_mem_fs());
    let dir = TempDir::new();
    let store = match durable::open_page_store(Some(Arc::new(fs.clone())), dir.path(), 1 << 30, 0) {
        Ok(store) => store,
        Err(e) => panic!("{e}"),
    };

    let tree = fill_tree(&store, 2000, 32);
    if let Err(e) = store.flush_tree(&tree) {
        panic!("{e}");
    }

    let gate = Arc::new(Stop::new());
    fs.set_gate(Some(gate.clone()));
    let (synced_tx, synced) = mpsc::sync_channel(1);
    let syncing = store.clone();
    spawn("sync", move || {
        let _ = synced_tx.send(syncing.sync());
    });

    while fs.syncs() == 0 {
        std::thread::sleep(Duration::from_millis(1));
    }

    store.set_limit(1);
    store.set_limit(1 << 30);

    let (done_tx, done) = mpsc::sync_channel::<Vec<usize>>(1);
    let reader = tree.clone();
    spawn("reads", move || {
        let mut missing = Vec::new();
        for i in (0..2000).step_by(97) {
            if reader.get(format!("key-{i:08}").as_bytes()).is_none() {
                missing.push(i);
            }
        }
        let _ = done_tx.send(missing);
    });

    match done.recv_timeout(Duration::from_secs(10)) {
        Ok(missing) => {
            if !missing.is_empty() {
                panic!("keys {missing:?} are missing");
            }
        }
        Err(_) => panic!("page reads wait for the sync of the checkpoint"),
    }

    gate.stop();
    match synced.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("{e}"),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn pages_are_synced_in_slices_with_a_pause() {
    let fs = GateFs::new(new_mem_fs());
    let dir = TempDir::new();
    let store = match durable::open_page_store(Some(Arc::new(fs.clone())), dir.path(), 1 << 30, 0) {
        Ok(store) => store,
        Err(e) => panic!("{e}"),
    };

    let pauses = Arc::new(AtomicI64::new(0));
    let counted = pauses.clone();
    store.pace(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Duration::from_millis(1)
    });

    let tree = fill_tree(&store, 60000, 256);
    if let Err(e) = store.flush_tree(&tree) {
        panic!("{e}");
    }

    if pauses.load(Ordering::SeqCst) < 2 {
        panic!(
            "a large flush should settle more than once, got {} pauses",
            pauses.load(Ordering::SeqCst)
        );
    }

    if fs.syncs() != pauses.load(Ordering::SeqCst) {
        panic!(
            "each slice is synced before its pause: {} syncs, {} pauses",
            fs.syncs(),
            pauses.load(Ordering::SeqCst)
        );
    }

    if let Err(e) = store.sync() {
        panic!("{e}");
    }
    store.set_limit(1);
    match tree.get(format!("key-{:08}", 59999).as_bytes()) {
        Some(v) if v.len() == 256 => {}
        other => panic!(
            "the last key reads back wrong: {} {}",
            other.as_ref().map_or(0, |v| v.len()),
            other.is_some()
        ),
    }
}
