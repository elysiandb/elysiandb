use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::fs::{Fs, FsError, filepath, or, read_file, write_file_atomic};

pub const MANIFEST_FILE: &str = "MANIFEST";
pub const PREVIOUS_MANIFEST: &str = "MANIFEST.prev";

pub struct Manifests {
    fs: Arc<dyn Fs>,
    dir: PathBuf,
    last: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredManifest {
    pub name: &'static str,
    pub data: Vec<u8>,
}

pub fn open_manifests(f: Option<Arc<dyn Fs>>, dir: &Path) -> Manifests {
    Manifests {
        fs: or(f),
        dir: dir.to_path_buf(),
        last: None,
    }
}

impl Manifests {
    pub fn stored(&self) -> Result<Vec<StoredManifest>, FsError> {
        let mut out = Vec::new();
        let mut first_err = None;
        for name in [MANIFEST_FILE, PREVIOUS_MANIFEST] {
            match read_file(&*self.fs, &filepath::join(&self.dir, name)) {
                Ok(data) => out.push(StoredManifest { name, data }),
                Err(e) if e.is_not_exist() => {}
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }

        if out.is_empty()
            && let Some(e) = first_err
        {
            return Err(e);
        }

        Ok(out)
    }

    pub fn adopt(&mut self, data: &[u8]) {
        self.last = if data.is_empty() {
            None
        } else {
            Some(data.to_vec())
        };
    }

    pub fn commit(&mut self, data: &[u8]) -> Result<(), FsError> {
        if let Some(last) = &self.last {
            write_bytes(
                &*self.fs,
                &filepath::join(&self.dir, PREVIOUS_MANIFEST),
                last,
            )?;
        }

        write_bytes(&*self.fs, &filepath::join(&self.dir, MANIFEST_FILE), data)?;

        self.adopt(data);

        Ok(())
    }
}

fn write_bytes(f: &dyn Fs, path: &Path, data: &[u8]) -> Result<(), FsError> {
    write_file_atomic(f, path, |w| w.write(data))
}
