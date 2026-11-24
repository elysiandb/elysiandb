use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::memory;
use crate::pagetree::Tree;
use crate::sync::{Mutex, spawn};

use super::engine::{ENGINE_LOG, Engine, panic_text};
use super::entity::{Slot, decode_slot_value};
use super::indexkey::key_seq;

pub const RETURN_MEMORY_ABOVE: i64 = 64 << 20;
pub const RETURN_MEMORY_EVERY: Duration = Duration::from_secs(60);
pub const CRITICAL_RETURN_EVERY: Duration = Duration::from_secs(30);
pub const WARM_BATCH: usize = 4096;

struct MemoryReturn {
    last: Option<Instant>,
    pending: bool,
}

static MEMORY_RETURN: Mutex<MemoryReturn> = Mutex::new(MemoryReturn {
    last: None,
    pending: false,
});

pub(crate) fn value_cache_limit(bytes: i64) -> i64 {
    if bytes <= 0 {
        return i64::MAX / 2;
    }

    bytes
}

impl Engine {
    pub(crate) fn release_values(&self, trees: Vec<Tree>) {
        let e = self.arc();
        let run = move || {
            let mut freed = 0i64;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                for t in &trees {
                    t.ascend_at(0, |k, v| {
                        let s = decode_slot_value(key_seq(k), v);
                        freed += e.cache.drop_lsn(s.lsn);
                        e.live.add(s.loc.segment, -i64::from(s.loc.length));
                        true
                    });
                }
            }));
            if let Err(payload) = outcome {
                let text = panic_text(payload.as_ref());
                ENGINE_LOG.warn(
                    "cannot release the values of a reset entity",
                    crate::kv!["err" => text],
                );
            }
            return_memory(freed);
        };

        if self.opts.background {
            drop(spawn("release-values", run));
            return;
        }

        run();
    }

    pub(crate) fn warm_values(&self) {
        for ent in self.all_entities() {
            let mut batch: Vec<Slot> = Vec::new();
            let flush = |batch: &mut Vec<Slot>| {
                crate::gocompat::sort::slice(batch, |a, b| {
                    if a.loc.segment != b.loc.segment {
                        return a.loc.segment < b.loc.segment;
                    }
                    a.loc.offset < b.loc.offset
                });
                for s in batch.iter() {
                    if self.closed.load(Ordering::SeqCst) {
                        return;
                    }
                    let _ = self.slot_bytes(s);
                }
                batch.clear();
            };

            ent.root.load().slots.ascend_at(0, |k, v| {
                batch.push(decode_slot_value(key_seq(k), v));
                if batch.len() >= WARM_BATCH {
                    flush(&mut batch);
                }
                !self.closed.load(Ordering::SeqCst)
            });
            flush(&mut batch);
        }
    }
}

pub(crate) fn return_memory(freed: i64) {
    if freed < RETURN_MEMORY_ABOVE {
        return;
    }

    let mut g = MEMORY_RETURN.lock();
    if g.pending {
        return;
    }

    let wait = match g.last {
        Some(last) => (last + RETURN_MEMORY_EVERY).saturating_duration_since(Instant::now()),
        None => Duration::ZERO,
    };
    if !wait.is_zero() {
        g.pending = true;
        drop(g);
        drop(spawn("return-memory", move || {
            std::thread::sleep(wait);
            {
                let mut g = MEMORY_RETURN.lock();
                g.pending = false;
                g.last = Some(Instant::now());
            }
            memory::free_os_memory();
        }));
        return;
    }

    g.last = Some(Instant::now());
    drop(g);
    memory::free_os_memory();
}

pub(crate) fn release_under_critical_pressure() {
    let due = {
        let mut g = MEMORY_RETURN.lock();
        let due = !g.pending
            && g.last
                .is_none_or(|last| last.elapsed() >= CRITICAL_RETURN_EVERY);
        if due {
            g.last = Some(Instant::now());
        }
        due
    };

    if due {
        drop(spawn("return-memory", memory::free_os_memory));
    }
}
