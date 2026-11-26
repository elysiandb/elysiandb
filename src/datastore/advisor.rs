use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::gocompat::sort as gosort;
use crate::memory::{self, Level};

use super::build::unix_nanos_now;
use super::engine::{Engine, IndexView};
use super::entity::{Entity, Root};
use super::indexkey::{
    INDEX_BITMAP, INDEX_FILTER, INDEX_REFS, IndexSpec, KEY_BOOL, KEY_ELEMENT, KEY_NUMBER, KEY_RAW,
    key_payload,
};

pub const ADVISOR_MIN_DOCS: usize = 4096;
pub const ADVISOR_FILL_SHARE: i64 = 20;
pub const BITMAP_MAX_DISTINCT: i64 = 64;
pub const ORDERED_MIN_DISTINCT: i64 = 1024;
pub const PRESSURE_RELEASE_IDLE_SECONDS: i64 = 10 * 60;
pub const PRESSURE_SWEEPS: i64 = 3;

const DISTINCT_KINDS: [u8; 4] = [KEY_RAW, KEY_NUMBER, KEY_BOOL, KEY_ELEMENT];

impl IndexView {
    pub(crate) fn distinct(&self, limit: i64) -> i64 {
        if self.bitmap() {
            let mut n = 0i64;
            for kind in DISTINCT_KINDS {
                let mut last: Vec<u8> = Vec::new();
                self.idx.tree.ascend_from(&[kind], |k, _| {
                    if k[0] != kind {
                        return false;
                    }
                    let value = &k[..k.len() - 4];
                    if value != last.as_slice() {
                        last = value.to_vec();
                        n += 1;
                    }
                    n <= limit
                });
                if n > limit {
                    return n;
                }
            }
            return n;
        }

        let mut n = 0i64;
        for kind in DISTINCT_KINDS {
            let mut from = vec![kind];
            while n <= limit {
                let mut found: Option<Vec<u8>> = None;
                self.idx.tree.ascend_from(&from, |k, _| {
                    if k[0] == kind {
                        found = Some(k.to_vec());
                    }
                    false
                });
                let Some(key) = found else {
                    break;
                };
                n += 1;
                from = key_payload(&key).to_vec();
                from.extend_from_slice(&[0xff; 5]);
            }
            if n > limit {
                return n;
            }
        }

        n
    }
}

impl Engine {
    pub fn advise(&self) {
        self.measure_indexes();

        for ent in self.all_entities() {
            let r = ent.root.load_full();
            let len = r.slots.len() as i64;
            let seen = ent.advised.swap(len, Ordering::SeqCst);
            if (len as usize) < ADVISOR_MIN_DOCS
                || (seen > 0 && len > seen + seen / ADVISOR_FILL_SHARE)
            {
                continue;
            }

            let mut fields: BTreeSet<Vec<u8>> = BTreeSet::new();
            for idx in r.indexes.values() {
                if idx.spec.kind == INDEX_FILTER || idx.spec.kind == INDEX_BITMAP {
                    fields.insert(idx.spec.field.clone());
                }
            }

            for field in fields {
                self.advise_filter(&ent, &r, &field);
            }
        }

        self.release_under_pressure();
    }

    fn advise_filter(&self, ent: &Arc<Entity>, r: &Root, field: &[u8]) {
        let ordered = IndexSpec::new(INDEX_FILTER, field);
        let bitmap = IndexSpec::new(INDEX_BITMAP, field);
        let o = r.indexes.get(&ordered.name());
        let b = r.indexes.get(&bitmap.name());
        let ordered_ready = o.is_some_and(|o| o.ready && !o.dormant);
        let bitmap_ready = b.is_some_and(|b| b.ready && !b.dormant);

        if ordered_ready && bitmap_ready {
            let Some(b) = b else { return };
            if IndexView::of(b.clone()).distinct(ORDERED_MIN_DISTINCT) > ORDERED_MIN_DISTINCT {
                self.drop_index(ent, &bitmap.name());
            } else {
                self.drop_index(ent, &ordered.name());
            }
        } else if ordered_ready && b.is_none() {
            let Some(o) = o else { return };
            if IndexView::of(o.clone()).distinct(BITMAP_MAX_DISTINCT) <= BITMAP_MAX_DISTINCT {
                let origin = ent.meta(&ordered.name()).origin();
                self.ensure_index_with(&ent.name, &bitmap, &origin);
            }
        } else if bitmap_ready && o.is_none() {
            let Some(b) = b else { return };
            if IndexView::of(b.clone()).distinct(ORDERED_MIN_DISTINCT) > ORDERED_MIN_DISTINCT {
                let origin = ent.meta(&bitmap.name()).origin();
                self.ensure_index_with(&ent.name, &ordered, &origin);
            }
        }
    }

    fn drop_index(&self, ent: &Arc<Entity>, name: &[u8]) {
        let mut st = ent.apply.lock();

        let r = ent.root.load_full();
        if !r.indexes.contains_key(name) {
            return;
        }

        let mut nr = (*r).clone();
        let mut indexes = (*r.indexes).clone();
        indexes.remove(name);
        nr.indexes = Arc::new(indexes);
        st.building.remove(name);
        self.forget_backlog(ent, name);
        ent.root.store(Arc::new(nr));
        ent.metas.write().remove(name);
        ent.usage.write().remove(name);
    }

    fn release_under_pressure(&self) {
        if memory::current() < Level::High {
            self.pressure_sweeps.store(0, Ordering::SeqCst);
            return;
        }

        let sweeps = self.pressure_sweeps.fetch_add(1, Ordering::SeqCst) + 1;
        if sweeps < PRESSURE_SWEEPS {
            return;
        }

        struct Candidate {
            ent: Arc<Entity>,
            name: Vec<u8>,
            uses: i64,
        }

        let cutoff = unix_nanos_now() - i128::from(PRESSURE_RELEASE_IDLE_SECONDS) * 1_000_000_000;
        let mut list: Vec<Candidate> = Vec::new();
        for ent in self.all_entities() {
            for (name, idx) in ent.root.load().indexes.iter() {
                if !idx.ready || idx.dormant || ent.pinned(&idx.spec) || idx.spec.kind == INDEX_REFS
                {
                    continue;
                }
                if let Some(last) = ent.last_use(name)
                    && i128::from(last) * 1_000_000_000 < cutoff
                {
                    list.push(Candidate {
                        ent: ent.clone(),
                        name: name.clone(),
                        uses: ent.meta(name).uses.load(Ordering::SeqCst),
                    });
                }
            }
        }

        gosort::slice(&mut list, |a, b| a.uses < b.uses);
        for c in list.iter().take(4) {
            self.release_index(&c.ent, &c.name);
        }
    }

    fn release_index(&self, ent: &Arc<Entity>, name: &[u8]) {
        let _g = ent.apply.lock();

        let r = ent.root.load_full();
        let Some(idx) = r.indexes.get(name) else {
            return;
        };
        if !idx.ready {
            return;
        }

        let mut nr = (*r).clone();
        let mut indexes = (*r.indexes).clone();
        indexes.insert(
            name.to_vec(),
            super::entity::Index::dormant(idx.spec.clone()),
        );
        nr.indexes = Arc::new(indexes);
        self.forget_backlog(ent, name);
        ent.root.store(Arc::new(nr));
    }
}
