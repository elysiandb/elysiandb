use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Weak};

use talos::ast::{Expression, Node, Property, walk_nodes};

use crate::datastore::{Engine, INDEX_BITMAP, INDEX_FILTER, IndexSpec, ORIGIN_HOOK};
use crate::engine;
use crate::query::Filters;
use crate::storage;
use crate::sync::Mutex;

use super::registry::{RegisteredHook, same_engine};
use super::view::VIEWS;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct IndexNeed {
    pub(crate) entity: String,
    pub(crate) field: String,
}

pub(crate) fn script_needs(script: &str) -> Vec<IndexNeed> {
    let Ok(program) = talos::parse("", script.as_bytes(), &[]) else {
        return Vec::new();
    };

    let mut seen: BTreeSet<IndexNeed> = BTreeSet::new();
    walk_nodes(Node::Program(&program), &mut |n| {
        let Node::CallExpression(call) = n else {
            return true;
        };

        let Expression::DotExpression(dot) = &call.callee else {
            return true;
        };
        if dot.identifier.name != "query" || call.argument_list.len() < 2 {
            return true;
        }

        let Expression::Identifier(id) = &dot.left else {
            return true;
        };
        if id.name != "ctx" {
            return true;
        }

        let Expression::StringLiteral(target) = &call.argument_list[0] else {
            return true;
        };

        let Expression::ObjectLiteral(object) = &call.argument_list[1] else {
            return true;
        };

        let target = target.value.string();
        for prop in &object.value {
            let Property::PropertyKeyed(keyed) = prop else {
                continue;
            };
            if keyed.computed {
                continue;
            }

            let field = match &keyed.key {
                Expression::StringLiteral(k) => k.value.string(),
                Expression::Identifier(k) => k.name.string(),
                _ => String::new(),
            };

            if !field.is_empty() && !engine::is_core_entity_type(target.as_bytes()) {
                seen.insert(IndexNeed {
                    entity: target.clone(),
                    field,
                });
            }
        }

        true
    });

    seen.into_iter().collect()
}

#[derive(Default)]
struct PreparedState {
    engine: Weak<Engine>,
    running: Weak<Engine>,
    pinned: BTreeSet<IndexNeed>,
    observed: BTreeMap<String, BTreeSet<IndexNeed>>,
}

pub(crate) struct PreparedIndexes {
    seen: Mutex<Option<HashSet<String>>>,
    st: Mutex<Option<PreparedState>>,
}

pub(crate) static PREPARED: PreparedIndexes = PreparedIndexes {
    seen: Mutex::new(None),
    st: Mutex::new(None),
};

fn filter_spec(field: &str) -> IndexSpec {
    IndexSpec::new(INDEX_FILTER, field.as_bytes())
}

impl PreparedIndexes {
    pub(crate) fn sync(&self, e: &Arc<Engine>, list: &[RegisteredHook]) {
        let mut g = self.st.lock();
        let p = g.get_or_insert_with(PreparedState::default);

        if !same_engine(&p.engine, e) {
            p.engine = Arc::downgrade(e);
            p.pinned = BTreeSet::new();
            p.observed = BTreeMap::new();
            *self.seen.lock() = None;
        }

        let mut scripts: HashSet<&str> = HashSet::new();
        let mut want: BTreeSet<IndexNeed> = BTreeSet::new();
        for h in list {
            if !h.hook.enabled {
                continue;
            }
            scripts.insert(&h.hook.script);
            for n in script_needs(&h.hook.script) {
                want.insert(n);
            }
        }

        p.observed
            .retain(|script, _| scripts.contains(script.as_str()));
        for needs in p.observed.values() {
            for n in needs {
                want.insert(n.clone());
            }
        }

        apply(p, e, &want);
        VIEWS.sync(e, list);

        if !same_engine(&p.running, e) {
            p.running = Arc::downgrade(e);
            let engine = e.clone();
            e.run_background(move |stop| VIEWS.maintain(&engine, &stop));
        }
    }

    pub(crate) fn observe(&self, script: &str, entity: &str, filters: &Filters) {
        let seen_key = |field: &[u8]| {
            let mut k = String::with_capacity(script.len() + entity.len() + field.len() + 2);
            k.push_str(script);
            k.push('\0');
            k.push_str(entity);
            k.push('\0');
            k.push_str(&String::from_utf8_lossy(field));
            k
        };

        let known = {
            let seen = self.seen.lock();
            filters
                .keys()
                .all(|f| seen.as_ref().is_some_and(|s| s.contains(&seen_key(f))))
        };
        if known {
            return;
        }

        let Some(e) = storage::engine() else {
            return;
        };

        {
            let mut seen = self.seen.lock();
            let s = seen.get_or_insert_with(HashSet::new);
            for field in filters.keys() {
                s.insert(seen_key(field));
            }
        }

        let mut g = self.st.lock();
        let p = g.get_or_insert_with(PreparedState::default);

        if !same_engine(&p.engine, &e) {
            return;
        }

        let mut added: BTreeSet<IndexNeed> = BTreeSet::new();
        let needs = p.observed.entry(script.to_string()).or_default();
        for field in filters.keys() {
            let n = IndexNeed {
                entity: entity.to_string(),
                field: String::from_utf8_lossy(field).into_owned(),
            };
            if needs.contains(&n) {
                continue;
            }
            needs.insert(n.clone());
            added.insert(n);
        }
        if needs.is_empty() {
            p.observed.remove(script);
        }

        if added.is_empty() {
            return;
        }

        let mut want: BTreeSet<IndexNeed> = p.pinned.clone();
        want.extend(added);

        apply(p, &e, &want);
    }
}

fn apply(p: &mut PreparedState, e: &Arc<Engine>, want: &BTreeSet<IndexNeed>) {
    let stale: Vec<IndexNeed> = p.pinned.difference(want).cloned().collect();
    for n in stale {
        e.unpin_index(n.entity.as_bytes(), &filter_spec(&n.field));
        p.pinned.remove(&n);
    }

    let mut fresh: Vec<IndexNeed> = Vec::new();
    for n in want {
        if !p.pinned.contains(n) {
            e.pin_index(n.entity.as_bytes(), &filter_spec(&n.field));
            p.pinned.insert(n.clone());
            fresh.push(n.clone());
        }
    }

    if fresh.is_empty() {
        return;
    }

    let e = e.clone();
    drop(crate::sync::spawn("hook-index-prepare", move || {
        for n in fresh {
            if (e.type_exists(n.entity.as_bytes()) || !e.snapshot(n.entity.as_bytes()).is_empty())
                && !has_filter_index(&e, &n)
            {
                e.ensure_index_with(
                    n.entity.as_bytes(),
                    &filter_spec(&n.field),
                    ORIGIN_HOOK.as_bytes(),
                );
            }
        }
    }));
}

fn has_filter_index(e: &Engine, n: &IndexNeed) -> bool {
    let snap = e.snapshot(n.entity.as_bytes());

    snap.has_index(&filter_spec(&n.field)) && !snap.index_dormant(&filter_spec(&n.field))
        || snap.has_index(&IndexSpec::new(INDEX_BITMAP, n.field.as_bytes()))
}
