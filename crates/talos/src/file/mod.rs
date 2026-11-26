#[allow(clippy::module_inception)]
mod file;

pub use file::{File, FileSet, Idx, Position, resolve_sourcemap_url};
