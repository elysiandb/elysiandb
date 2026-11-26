use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::memory::{self, Level};

use super::engine::{Engine, Wake};
use super::release::release_under_critical_pressure;

pub const DEFAULT_DIRTY_FLUSH_BYTES: i64 = 256 << 20;
pub const DIRTY_INTERVAL: Duration = Duration::from_millis(200);
pub const WRITER_WAIT_LIMIT: Duration = Duration::from_secs(30);

impl Engine {
    pub(crate) fn dirty_flush_bytes(&self) -> i64 {
        if self.opts.dirty_flush_bytes > 0 {
            return self.opts.dirty_flush_bytes
                + (memory::elastic() / 2).min(self.opts.dirty_flush_bytes);
        }

        DEFAULT_DIRTY_FLUSH_BYTES
    }

    pub fn dirty_bytes(&self) -> i64 {
        self.dirty.load(Ordering::SeqCst)
    }

    pub(crate) fn measure_dirty(&self) -> i64 {
        let mut n = 0i64;
        let mut count = |_: &std::sync::Arc<crate::pagetree::Ref>,
                         p: &std::sync::Arc<crate::pagetree::Page>|
         -> Result<(), crate::gocompat::errors::BoxError> {
            n += p.bytes();
            Ok(())
        };

        for ent in self.all_entities() {
            let r = ent.root.load_full();
            let _ = r.slots.for_each_dirty(&mut count);
            let _ = r.ids.for_each_dirty(&mut count);
            for idx in r.indexes.values() {
                if idx.ready {
                    let _ = idx.tree.for_each_dirty(&mut count);
                }
            }
        }

        self.dirty.store(n, Ordering::SeqCst);

        n
    }

    pub(crate) fn dirty_loop(&self) {
        self.every(DIRTY_INTERVAL, || {
            if self.measure_dirty() >= self.dirty_flush_bytes() {
                self.request_flush();
            }

            self.follow_pressure();
            self.check_disk();
        });
    }

    pub(crate) fn request_flush(&self) {
        self.bell.request_flush();
    }

    pub(crate) fn signal_flushed(&self) {
        self.bell.signal_flushed();
    }

    pub(crate) fn wait_for_dirty(&self) {
        let limit = 2 * self.dirty_flush_bytes();
        if !self.opts.background
            || self.dirty.load(Ordering::SeqCst) < limit
            || self.write_refusal().is_some()
        {
            return;
        }

        let deadline = Instant::now() + WRITER_WAIT_LIMIT;
        while self.dirty.load(Ordering::SeqCst) >= limit
            && !self.closed.load(Ordering::SeqCst)
            && Instant::now() < deadline
        {
            let seen = self.bell.flushed();
            self.request_flush();

            match self
                .bell
                .wait_flushed(seen, Instant::now() + DIRTY_INTERVAL)
            {
                Wake::Flush => {
                    self.measure_dirty();
                }
                Wake::Stop => return,
                Wake::Tick => {}
            }
        }
    }

    pub(crate) fn follow_pressure(&self) {
        let level = memory::current();
        let elastic = memory::elastic();

        if self.opts.cache_bytes > 0 {
            self.cache
                .set_limit(scaled(self.opts.cache_bytes + elastic / 4, level));
        }

        if self.opts.page_cache_bytes > 0 {
            self.pages
                .set_limit(scaled(self.opts.page_cache_bytes + elastic * 3 / 4, level));
        }

        if level == Level::Critical {
            release_under_critical_pressure();
        }
    }
}

pub(crate) fn scaled(base: i64, level: Level) -> i64 {
    if base <= 0 {
        return base;
    }

    match level {
        Level::High => base / 2,
        Level::Critical => base / 4,
        Level::Normal => base,
    }
}
