use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use arc_swap::ArcSwap;

use crate::docpath;
use crate::gocompat::errors::BoxError;
use crate::gocompat::strings;
use crate::hash::xxh64;
use crate::journal::{self, Kind, Location, Record};
use crate::ptree;
use crate::sync::{Mutex, RwLock, RwLockWriteGuard};

use super::engine::{Engine, Error};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KvEntry {
    pub(crate) value: Option<Vec<u8>>,
    pub(crate) expires_at: i64,
    pub(crate) lsn: u64,
    pub(crate) loc: Location,
}

pub(crate) type Shard = HashMap<Vec<u8>, Arc<KvEntry>>;
pub(crate) type ShardGuards<'a> = Vec<RwLockWriteGuard<'a, Shard>>;

pub(crate) struct KvStore {
    pub(crate) shards: Vec<RwLock<Shard>>,
    mask: u64,
    keys_mu: Mutex<()>,
    pub(crate) keys: ArcSwap<ptree::Tree<Vec<u8>>>,
    pub(crate) count: AtomicI64,
    pub(crate) with_ttl: AtomicI64,
    wheel: Mutex<BTreeMap<i64, BTreeSet<Vec<u8>>>>,
}

#[doc(hidden)]
pub fn lower_key(key: &[u8]) -> Vec<u8> {
    let mut out = strings::to_lower(key);
    out.push(0);
    out.extend_from_slice(key);
    out
}

pub(crate) fn opt_value(v: &[u8]) -> Option<Vec<u8>> {
    if v.is_empty() { None } else { Some(v.to_vec()) }
}

pub(crate) fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs_f64().ceil() as i64),
    }
}

impl KvStore {
    pub(crate) fn new(shards: i64) -> KvStore {
        let mut n: i64 = 1;
        while n < shards {
            n <<= 1;
        }

        KvStore {
            shards: (0..n).map(|_| RwLock::new(HashMap::new())).collect(),
            mask: (n - 1) as u64,
            keys_mu: Mutex::new(()),
            keys: ArcSwap::new(Arc::new(ptree::Tree::new())),
            count: AtomicI64::new(0),
            with_ttl: AtomicI64::new(0),
            wheel: Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) fn shard_index(&self, key: &[u8]) -> usize {
        (xxh64::sum64(key) & self.mask) as usize
    }

    pub(crate) fn shard(&self, key: &[u8]) -> &RwLock<Shard> {
        &self.shards[self.shard_index(key)]
    }

    pub(crate) fn lock_all(&self) -> ShardGuards<'_> {
        self.shards.iter().map(|s| s.write()).collect()
    }

    fn index_key(&self, key: &[u8], add: bool) {
        let _g = self.keys_mu.lock();
        let t = self.keys.load_full();
        let next = if add {
            t.put(&lower_key(key), key.to_vec()).0
        } else {
            t.delete(&lower_key(key)).0
        };
        self.keys.store(Arc::new(next));
    }

    fn schedule(&self, key: &[u8], expires_at: i64) {
        if expires_at <= 0 {
            return;
        }

        self.wheel
            .lock()
            .entry(expires_at)
            .or_default()
            .insert(key.to_vec());
    }

    pub(crate) fn set_locked(&self, sh: &mut Shard, key: &[u8], entry: KvEntry) {
        let expires_at = entry.expires_at;
        let old = sh.insert(key.to_vec(), Arc::new(entry));

        if old.is_none() {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.index_key(key, true);
        }

        let had_ttl = old.as_ref().is_some_and(|o| o.expires_at > 0);
        if expires_at > 0 && !had_ttl {
            self.with_ttl.fetch_add(1, Ordering::SeqCst);
        } else if expires_at <= 0 && had_ttl {
            self.with_ttl.fetch_sub(1, Ordering::SeqCst);
        }

        self.schedule(key, expires_at);
    }

    pub(crate) fn delete_locked(&self, sh: &mut Shard, key: &[u8]) -> bool {
        let Some(old) = sh.remove(key) else {
            return false;
        };

        self.count.fetch_sub(1, Ordering::SeqCst);
        if old.expires_at > 0 {
            self.with_ttl.fetch_sub(1, Ordering::SeqCst);
        }
        self.index_key(key, false);

        true
    }

    pub(crate) fn apply_put(&self, rec: &Record, replay: bool) {
        let mut sh = self.shard(&rec.key).write();

        let old = sh.get(&rec.key).cloned();
        if let Some(o) = &old
            && replay
            && o.lsn >= rec.lsn
        {
            return;
        }

        if rec.kind == journal::KIND_KV_MOVE {
            let Some(o) = old else {
                return;
            };

            let mut moved = (*o).clone();
            moved.lsn = rec.lsn;
            moved.loc = rec.loc;
            sh.insert(rec.key.clone(), Arc::new(moved));

            return;
        }

        if replay && rec.expires_at > 0 && rec.expires_at <= unix_now() {
            self.delete_locked(&mut sh, &rec.key);
            return;
        }

        self.set_locked(
            &mut sh,
            &rec.key,
            KvEntry {
                value: opt_value(&rec.value),
                expires_at: rec.expires_at,
                lsn: rec.lsn,
                loc: rec.loc,
            },
        );
    }

    pub(crate) fn apply_delete(&self, key: &[u8]) {
        let mut sh = self.shard(key).write();
        self.delete_locked(&mut sh, key);
    }

    pub(crate) fn clear_locked(&self, guards: &mut ShardGuards<'_>) {
        for g in guards.iter_mut() {
            **g = HashMap::new();
        }

        self.count.store(0, Ordering::SeqCst);
        self.with_ttl.store(0, Ordering::SeqCst);
        {
            let _g = self.keys_mu.lock();
            self.keys.store(Arc::new(ptree::Tree::new()));
        }
        *self.wheel.lock() = BTreeMap::new();
    }

    pub(crate) fn take_due(&self, now: i64) -> Vec<(i64, BTreeSet<Vec<u8>>)> {
        let mut wheel = self.wheel.lock();
        let due: Vec<i64> = wheel.range(..=now).map(|(&ts, _)| ts).collect();
        due.into_iter()
            .filter_map(|ts| wheel.remove(&ts).map(|b| (ts, b)))
            .collect()
    }
}

fn literal_prefix(pattern: &[u8]) -> &[u8] {
    match pattern.iter().position(|&c| c == b'*' || c == b'\\') {
        Some(i) => &pattern[..i],
        None => pattern,
    }
}

impl Engine {
    pub fn kv_get(&self, key: &[u8]) -> Option<(Option<Vec<u8>>, i64)> {
        let entry = self.kv.shard(key).read().get(key).cloned()?;
        Some((entry.value.clone(), entry.expires_at))
    }

    pub fn kv_expiration(&self, key: &[u8]) -> Option<i64> {
        let (_, exp) = self.kv_get(key)?;
        if exp <= 0 {
            return None;
        }

        Some(exp)
    }

    pub fn kv_put(&self, key: &[u8], value: &[u8], expires_at: i64) -> Result<(), BoxError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Box::new(Error::Closed));
        }

        if let Some(err) = self.write_refusal() {
            return Err(self.refuse(err));
        }

        let mut sh = self.kv.shard(key).write();
        let mut recs = vec![Record {
            kind: journal::KIND_KV_PUT,
            key: key.to_vec(),
            value: value.to_vec(),
            expires_at,
            ..Record::default()
        }];
        let ticket = match self.journal().append(&mut recs) {
            Ok(t) => t,
            Err(err) => return Err(Box::new(err)),
        };

        self.kv.set_locked(
            &mut sh,
            key,
            KvEntry {
                value: opt_value(value),
                expires_at,
                lsn: recs[0].lsn,
                loc: recs[0].loc,
            },
        );
        drop(sh);

        self.wait_durable(&ticket)
    }

    pub fn kv_delete(&self, key: &[u8]) -> bool {
        self.kv_delete_kind(key, journal::KIND_KV_DELETE, 0)
    }

    fn kv_delete_kind(&self, key: &[u8], kind: Kind, only_if_expires_at: i64) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }

        let mut sh = self.kv.shard(key).write();
        let Some(entry) = sh.get(key) else {
            return false;
        };
        if only_if_expires_at != 0 && entry.expires_at != only_if_expires_at {
            return false;
        }

        let mut recs = vec![Record {
            kind,
            key: key.to_vec(),
            ..Record::default()
        }];
        let Ok(ticket) = self.journal().append(&mut recs) else {
            return false;
        };

        self.kv.delete_locked(&mut sh, key);
        drop(sh);

        if kind == journal::KIND_KV_DELETE && self.wait_durable(&ticket).is_err() {
            return false;
        }

        true
    }

    pub fn kv_reset(&self) -> Result<(), BoxError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Box::new(Error::Closed));
        }

        let mut guards = self.kv.lock_all();
        let mut recs = vec![Record {
            kind: journal::KIND_KV_RESET,
            ..Record::default()
        }];
        let ticket = match self.journal().append(&mut recs) {
            Ok(t) => t,
            Err(err) => return Err(Box::new(err)),
        };

        self.kv.clear_locked(&mut guards);
        drop(guards);

        self.wait_durable(&ticket)
    }

    pub fn kv_count(&self) -> (i64, i64) {
        (
            self.kv.count.load(Ordering::SeqCst),
            self.kv.with_ttl.load(Ordering::SeqCst),
        )
    }

    pub fn kv_keys(&self) -> Vec<Vec<u8>> {
        let t = self.kv.keys.load_full();
        let mut keys = Vec::with_capacity(t.len());
        t.ascend_at(0, |_, key| {
            keys.push(key.clone());
            true
        });
        keys.sort();

        keys
    }

    pub fn kv_match(&self, pattern: &[u8], limit: i64, mut f: impl FnMut(&[u8]) -> bool) {
        let t = self.kv.keys.load_full();
        let lower = strings::to_lower(literal_prefix(pattern));
        let mut n = 0i64;
        t.ascend_from(&lower, |k, key| {
            if !k.starts_with(&lower) {
                return false;
            }

            if !docpath::match_glob(pattern, key) {
                return true;
            }

            n += 1;

            f(key) && (limit <= 0 || n < limit)
        });
    }

    pub(crate) fn expiry_loop(&self) {
        self.every(std::time::Duration::from_secs(1), || {
            self.expire_due(unix_now());
        });
    }

    pub fn expire_due(&self, now: i64) -> usize {
        let buckets = self.kv.take_due(now);

        let mut expired = 0usize;
        for (ts, keys) in buckets {
            for key in keys {
                if self.kv_delete_kind(&key, journal::KIND_KV_EXPIRE, ts) {
                    expired += 1;
                }
            }
        }

        if expired > 0
            && let Some(cb) = &self.opts.on_kv_expired
        {
            cb(expired);
        }

        expired
    }
}
