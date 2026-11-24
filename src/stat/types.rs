use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use crate::globals;
use crate::gocompat::json;
use crate::json_struct;
use crate::sync::RwLock;

pub struct StatsContainer {
    keys_count: AtomicU64,
    expiration_keys_count: AtomicU64,
    uptime_seconds: AtomicU64,
    total_requests: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    version: RwLock<String>,
    pub entities_count: AtomicI64,
}

impl Default for StatsContainer {
    fn default() -> Self {
        StatsContainer::empty()
    }
}

pub fn new_stats_container() -> StatsContainer {
    let s = StatsContainer::empty();
    *s.version.write() = globals::VERSION.to_string();
    s
}

fn decrement(v: &AtomicU64) {
    let _ = v.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
}

json_struct! {
    struct StatsDto {
        keys_count: u64 => "keys_count" [string],
        expiration_keys_count: u64 => "expiration_keys_count" [string],
        uptime_seconds: u64 => "uptime_seconds" [string],
        total_requests: u64 => "total_requests" [string],
        hits: u64 => "hits" [string],
        misses: u64 => "misses" [string],
        version: String => "version",
        entities_count: i64 => "entities_count" [string],
    }
}

impl StatsContainer {
    pub const fn empty() -> StatsContainer {
        StatsContainer {
            keys_count: AtomicU64::new(0),
            expiration_keys_count: AtomicU64::new(0),
            uptime_seconds: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            version: RwLock::new(String::new()),
            entities_count: AtomicI64::new(0),
        }
    }

    pub fn increment_keys_count(&self) {
        self.keys_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn increment_expiration_keys_count(&self) {
        self.expiration_keys_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn increment_total_requests(&self) {
        self.total_requests.fetch_add(1, Ordering::SeqCst);
    }

    pub fn increment_uptime_seconds(&self) {
        self.uptime_seconds.fetch_add(1, Ordering::SeqCst);
    }

    pub fn increment_hits(&self) {
        self.hits.fetch_add(1, Ordering::SeqCst);
    }

    pub fn increment_misses(&self) {
        self.misses.fetch_add(1, Ordering::SeqCst);
    }

    pub fn set_keys_count(&self, count: u64) {
        self.keys_count.store(count, Ordering::SeqCst);
    }

    pub fn set_expiration_keys_count(&self, count: u64) {
        self.expiration_keys_count.store(count, Ordering::SeqCst);
    }

    pub fn decrement_keys_count(&self) {
        decrement(&self.keys_count);
    }

    pub fn decrement_expiration_keys_count(&self) {
        decrement(&self.expiration_keys_count);
    }

    pub fn reset(&self) {
        self.keys_count.store(0, Ordering::SeqCst);
        self.expiration_keys_count.store(0, Ordering::SeqCst);
        self.uptime_seconds.store(0, Ordering::SeqCst);
        self.total_requests.store(0, Ordering::SeqCst);
        self.hits.store(0, Ordering::SeqCst);
        self.misses.store(0, Ordering::SeqCst);
    }

    pub(crate) fn replace(&self, other: StatsContainer) {
        self.keys_count
            .store(other.keys_count.load(Ordering::SeqCst), Ordering::SeqCst);
        self.expiration_keys_count.store(
            other.expiration_keys_count.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        self.uptime_seconds.store(
            other.uptime_seconds.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        self.total_requests.store(
            other.total_requests.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        self.hits
            .store(other.hits.load(Ordering::SeqCst), Ordering::SeqCst);
        self.misses
            .store(other.misses.load(Ordering::SeqCst), Ordering::SeqCst);
        *self.version.write() = other.version.read().clone();
        self.entities_count.store(
            other.entities_count.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
    }

    pub fn to_json(&self) -> String {
        let dto = StatsDto {
            keys_count: self.keys_count.load(Ordering::SeqCst),
            expiration_keys_count: self.expiration_keys_count.load(Ordering::SeqCst),
            uptime_seconds: self.uptime_seconds.load(Ordering::SeqCst),
            total_requests: self.total_requests.load(Ordering::SeqCst),
            hits: self.hits.load(Ordering::SeqCst),
            misses: self.misses.load(Ordering::SeqCst),
            version: self.version.read().clone(),
            entities_count: self.entities_count.load(Ordering::SeqCst),
        };

        let b = json::marshal(&dto).unwrap_or_default();

        String::from_utf8_lossy(&b).into_owned()
    }
}
