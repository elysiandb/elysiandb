use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::gocompat::json::Value;
use crate::sync::Mutex;

use super::engine::{Engine, Snapshot};
use super::entity::Draft;
use super::indexkey::filter_keys;

pub const CHANGE_LOG_SIZE: usize = 1 << 16;
pub const STAMP_SLOTS: usize = 1 << 14;
pub const MAX_WATCHED_FIELD: usize = 16;

#[derive(Clone, Copy, Default)]
struct Change {
    lsn: u64,
    seq: u32,
}

#[derive(Default)]
struct Ring {
    buf: Option<Vec<Change>>,
    next: usize,
    full: bool,
    covered: u64,
}

#[derive(Default)]
pub(crate) struct ChangeLog {
    mu: Mutex<Ring>,
}

impl ChangeLog {
    pub(crate) fn add(&self, since: u64, lsn: u64, seq: u32) {
        let mut g = self.mu.lock();
        let r = &mut *g;

        if r.buf.is_none() {
            r.buf = Some(vec![Change::default(); CHANGE_LOG_SIZE]);
            r.covered = since;
        }

        let Some(buf) = r.buf.as_mut() else {
            return;
        };

        if r.full {
            r.covered = r.covered.max(buf[r.next].lsn);
        }

        buf[r.next] = Change { lsn, seq };
        r.next += 1;
        if r.next == buf.len() {
            r.next = 0;
            r.full = true;
        }
    }

    pub(crate) fn reset(&self) {
        let mut g = self.mu.lock();
        g.buf = None;
        g.next = 0;
        g.full = false;
        g.covered = 0;
    }

    pub(crate) fn since(&self, from: u64, up_to: u64) -> Option<Vec<u32>> {
        let g = self.mu.lock();
        let buf = g.buf.as_ref()?;
        if from < g.covered {
            return None;
        }

        let (n, oldest) = if g.full {
            (buf.len(), g.next)
        } else {
            (g.next, 0)
        };

        let at = |i: usize| buf[(oldest + i) % buf.len()];
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if at(mid).lsn > from {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        let start = lo;

        let mut out = Vec::new();
        let mut seen: HashSet<u32> = HashSet::new();
        for i in start..n {
            let ch = at(i);
            if ch.lsn > up_to {
                break;
            }

            if n - start > 1 && !seen.insert(ch.seq) {
                continue;
            }

            out.push(ch.seq);
        }

        Some(out)
    }
}

pub(crate) struct FieldStamps {
    floor: AtomicU64,
    slots: Box<[AtomicU64]>,
}

impl FieldStamps {
    fn new(floor: u64) -> FieldStamps {
        FieldStamps {
            floor: AtomicU64::new(floor),
            slots: (0..STAMP_SLOTS).map(|_| AtomicU64::new(0)).collect(),
        }
    }
}

pub(crate) type Watched = BTreeMap<Vec<u8>, Arc<FieldStamps>>;

pub(crate) struct PendingStamp {
    table: Arc<FieldStamps>,
    slot: u32,
}

#[doc(hidden)]
pub fn stamp_slot(key: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in key {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }

    h % STAMP_SLOTS as u32
}

impl Snapshot {
    pub fn changed_since(&self, lsn: u64) -> Option<Vec<u32>> {
        let ent = self.ent.as_ref()?;
        ent.changes.since(lsn, self.r.version)
    }

    pub fn field_version(&self, field: &[u8], terms: &[&[u8]]) -> Option<u64> {
        let ent = self.ent.as_ref()?;
        let watched = ent.watched.load_full()?;
        let st = watched.get(field)?;

        let mut version = st.floor.load(Ordering::SeqCst);
        for term in terms {
            version = version.max(st.slots[stamp_slot(term) as usize].load(Ordering::SeqCst));
        }

        Some(version)
    }
}

impl Engine {
    pub fn watch_field(&self, name: &[u8], field: &[u8]) {
        let Some(ent) = self.entity(name, false) else {
            return;
        };

        let _g = ent.apply.lock();

        let current = ent.watched.load_full();
        if let Some(cur) = &current
            && (cur.contains_key(field) || cur.len() >= MAX_WATCHED_FIELD)
        {
            return;
        }

        let mut next: Watched = match &current {
            Some(cur) => (**cur).clone(),
            None => BTreeMap::new(),
        };

        next.insert(
            field.to_vec(),
            Arc::new(FieldStamps::new(ent.root.load().version)),
        );
        ent.watched.store(Some(Arc::new(next)));
    }
}

impl Draft<'_> {
    pub(crate) fn stamp_change(
        &mut self,
        old: Option<&Value>,
        new: Option<&Value>,
        old_known: bool,
    ) {
        let Some(watched) = self.ent.watched.load_full() else {
            return;
        };

        for (field, st) in watched.iter() {
            if !old_known {
                self.floors.push(st.clone());
            }

            for key in filter_keys(old, field) {
                self.stamps.push(PendingStamp {
                    table: st.clone(),
                    slot: stamp_slot(&key),
                });
            }

            for key in filter_keys(new, field) {
                self.stamps.push(PendingStamp {
                    table: st.clone(),
                    slot: stamp_slot(&key),
                });
            }
        }
    }

    pub(crate) fn apply_stamps(&mut self) {
        let lsn = self.r.version;
        for s in &self.stamps {
            s.table.slots[s.slot as usize].store(lsn, Ordering::SeqCst);
        }

        for st in &self.floors {
            st.floor.store(lsn, Ordering::SeqCst);
        }
    }
}
