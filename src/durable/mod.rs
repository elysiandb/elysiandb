pub mod disk;
pub mod fs;
pub mod kvfile;
pub mod manifest;

#[cfg(target_os = "linux")]
mod gentle_linux;
#[cfg(not(target_os = "linux"))]
mod gentle_other;
#[cfg(target_os = "linux")]
mod prealloc_linux;
#[cfg(not(target_os = "linux"))]
mod prealloc_other;

#[cfg(target_os = "linux")]
use prealloc_linux as prealloc;
#[cfg(not(target_os = "linux"))]
use prealloc_other as prealloc;

#[cfg(target_os = "linux")]
pub use gentle_linux::run_gently;
#[cfg(not(target_os = "linux"))]
pub use gentle_other::run_gently;

pub use disk::{IoStats, free_bytes, preallocate, stats, trim_preallocated};
pub use fs::{
    BufferedWriter, DirEntry, File, Fs, FsError, OS, OpenFlags, OsFile, OsFs, filepath, find_errno,
    or, read_file, write_file_atomic,
};
pub use kvfile::{ERR_CORRUPT_FILE, Error, KvRecord, read_kv_file, write_kv_file};
pub use manifest::{MANIFEST_FILE, Manifests, PREVIOUS_MANIFEST, StoredManifest, open_manifests};
