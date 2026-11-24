mod gunzip;
#[allow(clippy::module_inception)]
mod gzip;

pub use crate::archive::flate::BEST_SPEED;
pub use gunzip::{Header, Reader, new_reader};
pub use gzip::Writer;
