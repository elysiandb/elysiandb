#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use elysiandb::durable::{self, DirEntry, File, Fs, FsError, OpenFlags, filepath};

use super::tempdir::TempDir;

pub struct FailingState {
    inner: Arc<dyn Fs>,
    pub left: AtomicI64,
    pub counted: AtomicI64,
    failure: FsError,
}

#[derive(Clone)]
pub struct FailingFs(pub Arc<FailingState>);

impl FailingFs {
    pub fn new(inner: Arc<dyn Fs>, failure: FsError) -> FailingFs {
        FailingFs(Arc::new(FailingState {
            inner,
            left: AtomicI64::new(0),
            counted: AtomicI64::new(0),
            failure,
        }))
    }

    fn step(&self) -> Result<(), FsError> {
        self.0.counted.fetch_add(1, Ordering::SeqCst);
        if self.0.left.fetch_sub(1, Ordering::SeqCst) - 1 < 0 {
            return Err(self.0.failure.clone());
        }

        Ok(())
    }
}

impl Fs for FailingFs {
    fn open_file(
        &self,
        path: &Path,
        flags: OpenFlags,
        mode: u32,
    ) -> Result<Arc<dyn File>, FsError> {
        self.step()?;

        let file = self.0.inner.open_file(path, flags, mode)?;

        Ok(Arc::new(FailingFile {
            file,
            fs: self.clone(),
        }))
    }

    fn remove(&self, path: &Path) -> Result<(), FsError> {
        self.step()?;

        self.0.inner.remove(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        self.step()?;

        self.0.inner.rename(from, to)
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FsError> {
        self.0.inner.read_dir(path)
    }

    fn mkdir_all(&self, path: &Path, mode: u32) -> Result<(), FsError> {
        self.step()?;

        self.0.inner.mkdir_all(path, mode)
    }

    fn sync_dir(&self, path: &Path) -> Result<(), FsError> {
        self.step()?;

        self.0.inner.sync_dir(path)
    }
}

pub struct FailingFile {
    file: Arc<dyn File>,
    fs: FailingFs,
}

impl File for FailingFile {
    fn read_at(&self, buf: &mut [u8], off: u64) -> (usize, Option<FsError>) {
        self.file.read_at(buf, off)
    }

    fn write_at(&self, buf: &[u8], off: u64) -> (usize, Option<FsError>) {
        if let Err(e) = self.fs.step() {
            return (0, Some(e));
        }

        self.file.write_at(buf, off)
    }

    fn sync(&self) -> Result<(), FsError> {
        self.fs.step()?;

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

#[test]
fn test_preallocated_space_changes_nothing_in_a_file() {
    let dir = TempDir::new();
    let path = filepath::join(dir.path(), "segment");

    let f = match durable::OS.open_file(&path, OpenFlags::CREATE | OpenFlags::RDWR, 0o600) {
        Ok(f) => f,
        Err(e) => panic!("{e}"),
    };

    durable::preallocate(&*f, dir.path(), 1 << 20);
    if let Ok(size) = f.size()
        && size != 0
    {
        panic!("the visible size should not change, got {size}");
    }

    if let (_, Some(e)) = f.write_at(b"hello", 0) {
        panic!("{e}");
    }

    durable::trim_preallocated(&*f);

    let size = match f.size() {
        Ok(size) => size,
        Err(e) => panic!("{e}"),
    };
    if size != 5 {
        panic!("trimming should keep the data, size {size}");
    }

    let mut got = [0u8; 5];
    let (_, err) = f.read_at(&mut got, 0);
    if err.is_some() || &got != b"hello" {
        panic!("{:?} {err:?}", String::from_utf8_lossy(&got));
    }

    let _ = f.close();
}
