use std::sync::Once;
use std::time::Duration;

use crate::cache;
use crate::sync::spawn;

use super::init::config;

static CACHE_CLEANER: Once = Once::new();

pub fn boot_api_cache_cleaner() {
    if config().api.cache.enabled {
        CACHE_CLEANER.call_once(|| {
            drop(spawn("api-cache-cleaner", check_api_cache_periodically));
        });
    }
}

fn check_api_cache_periodically() {
    loop {
        cache::clean_expired_entries();
        std::thread::sleep(Duration::from_secs(1));
    }
}
