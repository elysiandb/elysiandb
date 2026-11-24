use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::durable::find_errno;
use crate::gocompat::errors::BoxError;
use crate::gocompat::json::{self, Map, Value};
use crate::journal::{self, Kind, Record, Ticket};
use crate::log;
use crate::sync::MutexGuard;

use super::engine::{ENGINE_LOG, Engine, Error, WRITE_FAILURES};
use super::entity::{DocRef, Entity, EntityState, decode_doc, decode_map};

pub const MAX_COMMIT_ATTEMPTS: usize = 64;
pub const FAILED_WRITE_WAIT: Duration = Duration::from_secs(2);
const RETRY_FIRST: Duration = Duration::from_millis(20);
const RETRY_MAX: Duration = Duration::from_secs(1);

struct TxOp {
    kind: Kind,
    entity: Vec<u8>,
    id: Vec<u8>,
    value: Vec<u8>,
    doc: DocRef,
}

struct Decoded {
    ptr: usize,
    len: usize,
    _keep: Option<Arc<[u8]>>,
    doc: Option<Arc<Map>>,
}

pub struct Tx<'e> {
    e: &'e Engine,
    reads: BTreeMap<Vec<u8>, u64>,
    ops: Vec<TxOp>,
    overlay: HashMap<Vec<u8>, usize>,
    resets: BTreeSet<Vec<u8>>,
    clear: bool,
    decoded: HashMap<Vec<u8>, Decoded>,
}

fn doc_key(entity: &[u8], id: &[u8]) -> Vec<u8> {
    let mut k = Vec::with_capacity(entity.len() + 1 + id.len());
    k.extend_from_slice(entity);
    k.push(0);
    k.extend_from_slice(id);
    k
}

fn split_doc_key(key: &[u8]) -> (&[u8], &[u8]) {
    match key.iter().position(|&c| c == 0) {
        Some(i) => (&key[..i], &key[i + 1..]),
        None => (key, b""),
    }
}

pub enum TxBytes<'a> {
    Overlay(&'a [u8]),
    Stored(Arc<[u8]>),
}

impl std::ops::Deref for TxBytes<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            TxBytes::Overlay(b) => b,
            TxBytes::Stored(b) => b,
        }
    }
}

impl Engine {
    fn new_tx(&self) -> Tx<'_> {
        Tx {
            e: self,
            reads: BTreeMap::new(),
            ops: Vec::new(),
            overlay: HashMap::new(),
            resets: BTreeSet::new(),
            clear: false,
            decoded: HashMap::new(),
        }
    }

    pub fn update<F>(&self, mut f: F) -> Result<(), BoxError>
    where
        F: FnMut(&mut Tx<'_>) -> Result<(), BoxError>,
    {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Box::new(Error::Closed));
        }

        self.writers.add(1);
        struct Done<'a>(&'a Engine);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.writers.done();
            }
        }
        let _done = Done(self);

        self.wait_for_dirty();
        self.wait_for_backlog();

        let mut attempt = 0usize;
        loop {
            let mut tx = self.new_tx();
            f(&mut tx)?;

            match self.commit(&mut tx) {
                Err(err)
                    if attempt < MAX_COMMIT_ATTEMPTS
                        && super::engine::is_error(err.as_ref(), Error::Conflict) =>
                {
                    self.stats.conflicts.fetch_add(1, Ordering::SeqCst);
                    if attempt > 4 {
                        std::thread::sleep(Duration::from_micros(attempt as u64 * 50));
                    }
                    attempt += 1;
                    continue;
                }
                Err(err) => return Err(err),
                Ok(ticket) => return self.wait_durable(&ticket),
            }
        }
    }

    pub(crate) fn wait_durable(&self, ticket: &Ticket) -> Result<(), BoxError> {
        let Err(mut err) = ticket.wait() else {
            return Ok(());
        };

        let (suppressed, ok) = WRITE_FAILURES.allow("log");
        if ok {
            ENGINE_LOG.error(
                "log write failed, writes are refused until the disk accepts them",
                &log::with_suppressed(crate::kv!["err" => log::display(&err)], suppressed),
            );
        }

        let mut delay = RETRY_FIRST;
        let deadline = Instant::now() + FAILED_WRITE_WAIT;
        while !self.closed.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(delay);
            delay = (delay * 2).min(RETRY_MAX);

            match self.journal().flush() {
                Ok(()) => match ticket.wait() {
                    Ok(()) => return Ok(()),
                    Err(e) => err = e,
                },
                Err(e) => err = e,
            }
        }

        if self.closed.load(Ordering::SeqCst) {
            return Err(Box::new(Error::Closed));
        }

        if find_errno(err.as_ref(), libc::ENOSPC) {
            return Err(self.refuse(Error::DiskFull));
        }

        Err(self.refuse(Error::WriteFailed))
    }

    fn commit(&self, tx: &mut Tx<'_>) -> Result<Ticket, BoxError> {
        if tx.ops.is_empty() {
            return Ok(Ticket::default());
        }

        if let Some(err) = self.write_refusal()
            && (tx.adds_data() || self.journal().failure().is_some())
        {
            return Err(self.refuse(err));
        }

        let mut names: BTreeSet<Vec<u8>> = BTreeSet::new();
        for op in &tx.ops {
            if !op.entity.is_empty()
                && op.kind != journal::KIND_ENTITY_CREATE
                && op.kind != journal::KIND_ENTITY_DROP
            {
                names.insert(op.entity.clone());
            }
        }

        if tx.clear {
            for name in self.catalog.read().entities.keys() {
                names.insert(name.clone());
            }
        }

        let sorted: Vec<Vec<u8>> = names.into_iter().collect();

        if self.opts.background && tx.ops.len() >= super::backlog::BULK_BATCH_RECORDS {
            self.read_ahead(tx);
        }

        let ents: Vec<Arc<Entity>> = sorted.iter().map(|n| self.entity_or_create(n)).collect();
        let mut guards: Vec<MutexGuard<'_, EntityState>> = Vec::with_capacity(ents.len());
        for ent in &ents {
            guards.push(ent.apply.lock());
        }

        let type_ops = tx.clear
            || tx.ops.iter().any(|op| {
                op.kind == journal::KIND_ENTITY_CREATE || op.kind == journal::KIND_ENTITY_DROP
            });

        let types_guard = if type_ops {
            Some(self.types_mu.lock())
        } else {
            None
        };

        let unlock = |types_guard: Option<MutexGuard<'_, ()>>,
                      guards: Vec<MutexGuard<'_, EntityState>>| {
            drop(types_guard);
            for g in guards.into_iter().rev() {
                drop(g);
            }
        };

        for (key, &lsn) in &tx.reads {
            let (entity, id) = split_doc_key(key);
            let mut current = 0u64;
            if let Some(ent) = self.entity(entity, false)
                && let Some(s) = ent.root.load().slot_by_id(id)
            {
                current = s.lsn;
            }

            if current != lsn {
                unlock(types_guard, guards);
                return Err(Box::new(Error::Conflict));
            }
        }

        let mut gens: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
        let gen_of = |name: &[u8], gens: &mut BTreeMap<Vec<u8>, u64>| -> u64 {
            if let Some(&g) = gens.get(name) {
                return g;
            }
            let g = self.entity_or_create(name).root.load().gen_;
            gens.insert(name.to_vec(), g);
            g
        };

        let mut recs: Vec<Record> = Vec::with_capacity(tx.ops.len());
        let mut docs: Vec<DocRef> = Vec::with_capacity(tx.ops.len());
        let mut types = self.types();

        for op in &tx.ops {
            match op.kind {
                journal::KIND_DOC_PUT => {
                    recs.push(Record {
                        kind: op.kind,
                        entity: op.entity.clone(),
                        key: op.id.clone(),
                        value: op.value.clone(),
                        r#gen: gen_of(&op.entity, &mut gens),
                        ..Record::default()
                    });
                    docs.push(op.doc.clone());
                }
                journal::KIND_DOC_DELETE => {
                    recs.push(Record {
                        kind: op.kind,
                        entity: op.entity.clone(),
                        key: op.id.clone(),
                        r#gen: gen_of(&op.entity, &mut gens),
                        ..Record::default()
                    });
                    docs.push(None);
                }
                journal::KIND_ENTITY_RESET => {
                    let g = gen_of(&op.entity, &mut gens).wrapping_add(1);
                    gens.insert(op.entity.clone(), g);
                    recs.push(Record {
                        kind: op.kind,
                        entity: op.entity.clone(),
                        r#gen: g,
                        ..Record::default()
                    });
                    docs.push(None);
                }
                journal::KIND_ENTITY_CREATE => {
                    if types.contains(&op.entity) {
                        continue;
                    }
                    types.push(op.entity.clone());
                    recs.push(Record {
                        kind: op.kind,
                        entity: op.entity.clone(),
                        ..Record::default()
                    });
                    docs.push(None);
                }
                journal::KIND_ENTITY_DROP => {
                    if !types.contains(&op.entity) {
                        continue;
                    }
                    types.retain(|t| *t != op.entity);
                    recs.push(Record {
                        kind: op.kind,
                        entity: op.entity.clone(),
                        ..Record::default()
                    });
                    docs.push(None);
                }
                journal::KIND_CLEAR_ALL => {
                    recs.push(Record {
                        kind: journal::KIND_KV_RESET,
                        ..Record::default()
                    });
                    docs.push(None);
                    for name in &sorted {
                        let g = gen_of(name, &mut gens).wrapping_add(1);
                        gens.insert(name.clone(), g);
                        recs.push(Record {
                            kind: journal::KIND_ENTITY_RESET,
                            entity: name.clone(),
                            r#gen: g,
                            ..Record::default()
                        });
                        docs.push(None);
                    }
                    for t in &types {
                        recs.push(Record {
                            kind: journal::KIND_ENTITY_DROP,
                            entity: t.clone(),
                            ..Record::default()
                        });
                        docs.push(None);
                    }
                    types.clear();
                }
                _ => {}
            }
        }

        if recs.is_empty() {
            unlock(types_guard, guards);
            return Ok(Ticket::default());
        }

        let mut kv_guards = if tx.clear {
            Some(self.kv.lock_all())
        } else {
            None
        };

        let ticket = match self.journal().append(&mut recs) {
            Ok(t) => t,
            Err(err) => {
                unlock(types_guard, guards);
                return Err(Box::new(err));
            }
        };

        {
            let mut held: Vec<(&Arc<Entity>, &mut EntityState)> = ents
                .iter()
                .zip(guards.iter_mut())
                .map(|(ent, g)| (ent, &mut **g))
                .collect();
            self.apply_batch(&recs, Some(&docs), false, &mut held, kv_guards.as_mut());
        }
        unlock(types_guard, guards);
        drop(kv_guards);

        Ok(ticket)
    }
}

impl<'e> Tx<'e> {
    pub fn read_bytes(&mut self, entity: &[u8], id: &[u8]) -> Option<TxBytes<'_>> {
        let key = doc_key(entity, id);
        if let Some(&i) = self.overlay.get(&key) {
            if self.ops[i].kind == journal::KIND_DOC_DELETE {
                return None;
            }
            return Some(TxBytes::Overlay(&self.ops[i].value));
        }

        if self.resets.contains(entity) || self.clear {
            return None;
        }

        let Some(doc) = self.e.snapshot(entity).get(id) else {
            self.reads.entry(key).or_insert(0);
            return None;
        };

        let lsn = doc.lsn();
        self.reads.entry(key).or_insert(lsn);

        doc.bytes().ok().map(TxBytes::Stored)
    }

    pub fn read_shared(&mut self, entity: &[u8], id: &[u8]) -> Option<Arc<Map>> {
        let key = doc_key(entity, id);
        let (ptr, len, keep) = {
            let b = self.read_bytes(entity, id)?;
            let keep: Option<Arc<[u8]>> = match &b {
                TxBytes::Stored(a) => Some(a.clone()),
                TxBytes::Overlay(_) => None,
            };
            (b.as_ptr() as usize, b.len(), keep)
        };

        if let Some(c) = self.decoded.get(&key)
            && c.len == len
            && len > 0
            && c.ptr == ptr
        {
            return c.doc.clone();
        }

        let decoded = match &keep {
            Some(a) => decode_map(a),
            None => {
                let i = self.overlay[&key];
                decode_map(&self.ops[i].value)
            }
        };
        let doc = match decoded {
            Ok(Some(m)) => Some(Arc::new(m)),
            _ => None,
        };

        self.decoded.insert(
            key,
            Decoded {
                ptr,
                len,
                _keep: keep,
                doc: doc.clone(),
            },
        );

        doc
    }

    pub fn read(&mut self, entity: &[u8], id: &[u8]) -> Option<Map> {
        let b = self.read_bytes(entity, id)?;
        match decode_map(&b) {
            Ok(Some(m)) => Some(m),
            _ => None,
        }
    }

    pub fn exists(&mut self, entity: &[u8], id: &[u8]) -> bool {
        self.read_bytes(entity, id).is_some()
    }

    pub fn put(&mut self, entity: &[u8], id: &[u8], doc: &Map) -> Result<(), BoxError> {
        self.put_opt(entity, id, Some(doc))
    }

    pub fn put_opt(&mut self, entity: &[u8], id: &[u8], doc: Option<&Map>) -> Result<(), BoxError> {
        let value = json::marshal(&doc)?;
        let decoded = decode_doc(&value);
        self.put_value(entity, id, value, decoded);
        Ok(())
    }

    pub fn put_bytes(
        &mut self,
        entity: &[u8],
        id: &[u8],
        value: Vec<u8>,
        doc: Option<Map>,
    ) -> Result<(), BoxError> {
        self.put_value(entity, id, value, doc.map(|m| Arc::new(Value::Object(m))));
        Ok(())
    }

    fn put_value(&mut self, entity: &[u8], id: &[u8], value: Vec<u8>, doc: DocRef) {
        self.record(TxOp {
            kind: journal::KIND_DOC_PUT,
            entity: entity.to_vec(),
            id: id.to_vec(),
            value,
            doc,
        });
    }

    pub fn delete(&mut self, entity: &[u8], id: &[u8]) {
        self.record(TxOp {
            kind: journal::KIND_DOC_DELETE,
            entity: entity.to_vec(),
            id: id.to_vec(),
            value: Vec::new(),
            doc: None,
        });
    }

    fn record(&mut self, op: TxOp) {
        let key = doc_key(&op.entity, &op.id);
        self.ops.push(op);
        self.overlay.insert(key, self.ops.len() - 1);
    }

    pub fn reset_entity(&mut self, entity: &[u8]) {
        let ops = &self.ops;
        self.overlay.retain(|_, i| ops[*i].entity != entity);

        self.ops.push(TxOp {
            kind: journal::KIND_ENTITY_RESET,
            entity: entity.to_vec(),
            id: Vec::new(),
            value: Vec::new(),
            doc: None,
        });
        self.resets.insert(entity.to_vec());
    }

    pub fn ensure_type(&mut self, entity: &[u8]) {
        self.ops.push(TxOp {
            kind: journal::KIND_ENTITY_CREATE,
            entity: entity.to_vec(),
            id: Vec::new(),
            value: Vec::new(),
            doc: None,
        });
    }

    pub fn drop_type(&mut self, entity: &[u8]) {
        self.ops.push(TxOp {
            kind: journal::KIND_ENTITY_DROP,
            entity: entity.to_vec(),
            id: Vec::new(),
            value: Vec::new(),
            doc: None,
        });
    }

    pub fn clear_all(&mut self) {
        self.ops.push(TxOp {
            kind: journal::KIND_CLEAR_ALL,
            entity: Vec::new(),
            id: Vec::new(),
            value: Vec::new(),
            doc: None,
        });
        self.overlay.clear();
        self.clear = true;
    }

    pub fn type_exists(&self, entity: &[u8]) -> bool {
        let mut exists = self.e.type_exists(entity);
        for op in &self.ops {
            if op.kind == journal::KIND_ENTITY_CREATE && op.entity == entity {
                exists = true;
            } else if (op.kind == journal::KIND_ENTITY_DROP && op.entity == entity)
                || op.kind == journal::KIND_CLEAR_ALL
            {
                exists = false;
            }
        }

        exists
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    fn adds_data(&self) -> bool {
        self.ops
            .iter()
            .any(|op| op.kind == journal::KIND_DOC_PUT || op.kind == journal::KIND_ENTITY_CREATE)
    }

    pub(crate) fn doc_ops(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.ops
            .iter()
            .filter(|op| op.kind == journal::KIND_DOC_PUT || op.kind == journal::KIND_DOC_DELETE)
            .map(|op| (op.entity.as_slice(), op.id.as_slice()))
    }
}
