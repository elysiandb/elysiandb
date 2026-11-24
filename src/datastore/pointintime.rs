use std::collections::BTreeMap;
use std::sync::{Arc, Once};

use crate::gocompat::time::Time;
use crate::sync::MutexGuard;

use super::engine::{Engine, Snapshot};
use super::entity::EntityState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KvItem {
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>,
    pub expires_at: i64,
}

pub struct PointInTime {
    pub snapshots: BTreeMap<Vec<u8>, Snapshot>,
    pub types: Vec<Vec<u8>>,
    pub kv: Vec<KvItem>,
    pub lsn: u64,
    pub at: Time,

    e: Arc<Engine>,
    once: Once,
}

impl Engine {
    pub fn point_in_time(&self) -> PointInTime {
        self.journal().hold();
        self.pages.hold();

        loop {
            let ents = self.all_entities();
            let guards: Vec<MutexGuard<'_, EntityState>> =
                ents.iter().map(|ent| ent.apply.lock()).collect();
            let types_guard = self.types_mu.lock();

            if self.all_entities().len() != ents.len() {
                drop(types_guard);
                for g in guards.into_iter().rev() {
                    drop(g);
                }
                continue;
            }

            let mut p = PointInTime {
                snapshots: BTreeMap::new(),
                types: self.types(),
                kv: Vec::new(),
                lsn: self.journal().next_lsn().wrapping_sub(1),
                at: Time::now(),
                e: self.arc(),
                once: Once::new(),
            };
            for ent in &ents {
                p.snapshots.insert(
                    ent.name.clone(),
                    Snapshot::new(
                        self.arc(),
                        Some(ent.clone()),
                        ent.root.load_full(),
                        &ent.name,
                    ),
                );
            }

            {
                let kv_guards = self.kv.lock_all();
                p.kv.reserve(
                    self.kv
                        .count
                        .load(std::sync::atomic::Ordering::SeqCst)
                        .max(0) as usize,
                );
                for sh in kv_guards.iter() {
                    for (key, entry) in sh.iter() {
                        p.kv.push(KvItem {
                            key: key.clone(),
                            value: entry.value.clone(),
                            expires_at: entry.expires_at,
                        });
                    }
                }
                for g in kv_guards.into_iter().rev() {
                    drop(g);
                }
            }

            drop(types_guard);
            for g in guards.into_iter().rev() {
                drop(g);
            }

            p.kv.sort_by(|a, b| a.key.cmp(&b.key));

            return p;
        }
    }
}

impl PointInTime {
    pub fn release(&self) {
        self.once.call_once(|| {
            self.e.journal().release();
            self.e.pages.release();
        });
    }
}
