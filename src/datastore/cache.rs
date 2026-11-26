use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use crate::sync::RwLock;

pub const CACHE_ENTRY_OVERHEAD: i64 = 112;

pub(crate) struct ValueCache {
    limit: AtomicI64,
    shards: Vec<RwLock<CacheShard>>,
    pub(crate) hits: AtomicI64,
    pub(crate) misses: AtomicI64,
}

struct CacheShard {
    index: HashMap<u64, usize>,
    entries: Vec<CacheEntry>,
    free: Vec<usize>,
    hand: usize,
    bytes: i64,
    limit: i64,
}

#[derive(Default)]
struct CacheEntry {
    lsn: u64,
    data: Arc<[u8]>,
    referenced: AtomicBool,
    used: bool,
}

fn entry_size(data: &[u8]) -> i64 {
    data.len() as i64 + CACHE_ENTRY_OVERHEAD
}

impl ValueCache {
    pub(crate) fn new(limit: i64, shards: usize) -> ValueCache {
        let shards = shards.max(1);
        let per = (limit / shards as i64).max(1);
        ValueCache {
            limit: AtomicI64::new(limit),
            shards: (0..shards)
                .map(|_| {
                    RwLock::new(CacheShard {
                        index: HashMap::new(),
                        entries: Vec::new(),
                        free: Vec::new(),
                        hand: 0,
                        bytes: 0,
                        limit: per,
                    })
                })
                .collect(),
            hits: AtomicI64::new(0),
            misses: AtomicI64::new(0),
        }
    }

    pub(crate) fn set_limit(&self, limit: i64) {
        if self.limit.swap(limit, Ordering::SeqCst) == limit {
            return;
        }

        let per = (limit / self.shards.len() as i64).max(1);
        for s in &self.shards {
            let mut s = s.write();
            s.limit = per;
            while s.bytes > s.limit && !s.index.is_empty() {
                s.evict_one();
            }
        }
    }

    fn shard(&self, lsn: u64) -> &RwLock<CacheShard> {
        let h = lsn.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32;
        &self.shards[(h % self.shards.len() as u64) as usize]
    }

    pub(crate) fn get(&self, lsn: u64) -> Option<Arc<[u8]>> {
        let found = {
            let s = self.shard(lsn).read();
            s.index.get(&lsn).map(|&i| {
                let e = &s.entries[i];
                if !e.referenced.load(Ordering::Relaxed) {
                    e.referenced.store(true, Ordering::Relaxed);
                }
                e.data.clone()
            })
        };

        if found.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }

        found
    }

    pub(crate) fn peek(&self, lsn: u64) -> Option<Arc<[u8]>> {
        let s = self.shard(lsn).read();
        s.index.get(&lsn).map(|&i| s.entries[i].data.clone())
    }

    pub(crate) fn put(&self, lsn: u64, data: Arc<[u8]>) {
        let mut s = self.shard(lsn).write();

        if s.index.contains_key(&lsn) {
            return;
        }

        let size = entry_size(&data);
        if size > s.limit {
            return;
        }

        while s.bytes + size > s.limit && !s.index.is_empty() {
            s.evict_one();
        }

        let i = match s.free.pop() {
            Some(i) => i,
            None => {
                s.entries.push(CacheEntry::default());
                s.entries.len() - 1
            }
        };

        s.entries[i] = CacheEntry {
            lsn,
            data,
            referenced: AtomicBool::new(false),
            used: true,
        };
        s.index.insert(lsn, i);
        s.bytes += size;
    }

    pub(crate) fn drop_lsn(&self, lsn: u64) -> i64 {
        let mut s = self.shard(lsn).write();

        let Some(i) = s.index.remove(&lsn) else {
            return 0;
        };

        let size = entry_size(&s.entries[i].data);
        s.bytes -= size;
        s.entries[i] = CacheEntry::default();
        s.free.push(i);

        size
    }

    pub(crate) fn size(&self) -> i64 {
        self.shards.iter().map(|s| s.write().bytes).sum()
    }
}

impl CacheShard {
    fn evict_one(&mut self) {
        loop {
            if self.hand >= self.entries.len() {
                self.hand = 0;
            }

            let hand = self.hand;
            let e = &mut self.entries[hand];
            if !e.used {
                self.hand += 1;
                continue;
            }

            if *e.referenced.get_mut() {
                *e.referenced.get_mut() = false;
                self.hand += 1;
                continue;
            }

            let lsn = e.lsn;
            let size = entry_size(&e.data);
            *e = CacheEntry::default();
            self.index.remove(&lsn);
            self.bytes -= size;
            self.free.push(hand);
            self.hand += 1;

            return;
        }
    }
}
