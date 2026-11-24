#[allow(clippy::module_inception)]
mod stat;
mod types;

pub use stat::{STATS, init, stats};
pub use types::{StatsContainer, new_stats_container};
