mod counting;
#[allow(clippy::module_inception)]
mod memory;

pub use counting::{CountingAllocator, allocated, free_os_memory, resident};
pub use memory::{
    BUILDS, Busy, DIRTY, ERR_BUSY, HOOK_RESULTS, Level, PAGES, Pool, REQUESTS, VALUES, budget,
    configure, current, elastic, follow_spare, level_for, live, on_level, pools, request_cap,
    sample, set_elastic, set_level, start, stop, total,
};
