#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use elysiandb::durable::{DirEntry, File, Fs, FsError, OpenFlags};
use elysiandb::sync::{Mutex, Stop};

use super::memfs_test::MemFs;

pub struct GateState {
    mem: MemFs,
    gate: Mutex<Option<Arc<Stop>>>,
    pub syncs: AtomicI64,
}

#[derive(Clone)]
pub struct GateFs(pub Arc<GateState>);

impl GateFs {
    pub fn new(mem: MemFs) -> GateFs {
        GateFs(Arc::new(GateState {
            mem,
            gate: Mutex::new(None),
            syncs: AtomicI64::new(0),
        }))
    }

    pub fn mem(&self) -> &MemFs {
        &self.0.mem
    }

    pub fn set_gate(&self, gate: Option<Arc<Stop>>) {
        *self.0.gate.lock() = gate;
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
