#[allow(clippy::module_inception)]
mod uuid;

pub use uuid::{format, new_v4, random_bytes};
