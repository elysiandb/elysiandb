use std::path::Path;
use std::sync::Arc;

use crate::configuration::Config;
use crate::gocompat::strconv;
use crate::gocompat::strings::trim_space;
use crate::kv;
use crate::memory;
use crate::storage;

use super::init::{BOOT_LOG, config};

const AUTO_MEMORY_SHARE: f64 = 0.75;
const CONTAINER_SHARE: f64 = 0.90;
const UNLIMITED_CGROUPS: i64 = 1 << 60;

pub const CGROUP_LIMIT_FILES: [&str; 2] = [
    "/sys/fs/cgroup/memory.max",
    "/sys/fs/cgroup/memory/memory.limit_in_bytes",
];

pub fn container_memory_limit() -> i64 {
    container_memory_limit_from(&CGROUP_LIMIT_FILES.map(Path::new))
}

pub fn container_memory_limit_from(paths: &[&Path]) -> i64 {
    for path in paths {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };

        let v = trim_space(&data);
        if v == b"max" {
            return 0;
        }

        if let Ok(n) = strconv::parse_int(v, 10, 64)
            && n > 0
            && n < UNLIMITED_CGROUPS
        {
            return n;
        }
    }

    0
}

pub fn budget_within_container(budget: i64, container: i64) -> i64 {
    if container <= 0 || budget as f64 <= container as f64 * CONTAINER_SHARE {
        return budget;
    }

    BOOT_LOG.warn(
        "the memory budget is above the container limit, it is lowered",
        kv!["limit_mb" => budget >> 20, "container_mb" => container >> 20],
    );

    (container as f64 * CONTAINER_SHARE) as i64
}

pub fn memory_budget(cfg: &Config) -> (i64, bool) {
    if cfg.store.memory.limit_mb > 0 {
        return (
            budget_within_container(cfg.store.memory.limit_mb << 20, container_memory_limit()),
            true,
        );
    }

    let limit = container_memory_limit();
    if limit > 0 {
        return ((limit as f64 * AUTO_MEMORY_SHARE) as i64, false);
    }

    (0, false)
}

pub fn apply_memory_limit() {
    let cfg = config();
    let (budget, explicit) = memory_budget(&cfg);
    if !explicit {
        memory::configure(0);
    }

    if budget <= 0 {
        return;
    }

    if explicit {
        memory::configure(budget);
        memory::start();
        let mut hooks = storage::hooks();
        hooks.value_cache_bytes = Some(Arc::new(|_: &Config| memory::VALUES.limit()));
        hooks.page_cache_bytes = Some(Arc::new(|_: &Config| memory::PAGES.limit()));
        hooks.dirty_flush_bytes = Some(Arc::new(|_: &Config| memory::DIRTY.limit()));
        storage::set_hooks(hooks);
    }

    BOOT_LOG.info(
        "memory budget applied",
        kv![
            "limit_mb" => budget >> 20,
            "explicit" => explicit,
            "documents_cached_on_demand" => explicit,
        ],
    );
}
