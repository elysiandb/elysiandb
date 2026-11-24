use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::durable::{self, Fs, FsError, KvRecord, Manifests, filepath, split_page_id};
use crate::gocompat::binary;
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json;
use crate::gocompat::strconv;
use crate::gocompat::time::{self as gotime, Time};
use crate::hash::crc32c;
use crate::journal::{Location, Position};
use crate::json_struct;
use crate::log;
use crate::pagetree::{Ref, Token, Tree};
use crate::sync::MutexGuard;

use super::engine::{ENGINE_LOG, Engine, MANIFEST_FILE};
use super::entity::{Entity, Index, Indexes, Root, Slot, encode_slot};
use super::indexkey::{IndexSpec, RefPath, parse_index_name, seq_key};
use super::indexmeta::ORIGIN_QUERY;
use super::kv::KvEntry;
use super::release::{RETURN_MEMORY_ABOVE, return_memory};

pub const CHECKPOINT_MAGIC: &[u8] = b"ELYCKPT1";
pub const CHECKPOINT_VERSION: u64 = 1;
pub const MANIFEST_FORMAT: i64 = 1;
pub const MANIFEST_FORMAT_PAGES: i64 = 2;
pub const LEGACY_CHECKPOINTS: &str = "legacy-checkpoint";
pub const PAGE_WALK_EVERY: Duration = Duration::from_secs(15);
pub const PAGE_VICTIMS_PER_CYCLE: usize = 4;
pub const RELOCATE_BATCH: usize = 256;
pub const RECLAIM_ABOVE_BYTES: i64 = 128 << 20;
pub const RECLAIM_QUIET_LOG: i64 = 16 << 20;
pub const COMPACT_PAGES_ROUNDS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointCorrupt;

impl fmt::Display for CheckpointCorrupt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("checkpoint is corrupt")
    }
}

impl std::error::Error for CheckpointCorrupt {}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct PageRoot {
        pub id: u64 => "id",
        pub size: u32 => "size",
    }
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct ManifestIndex {
        pub name: Vec<u8> => "name",
        pub root: PageRoot => "root",
        pub dormant: bool => "dormant" [omitempty],
    }
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct ManifestEntity {
        pub name: Vec<u8> => "name",
        pub gen_: u64 => "gen",
        pub next_seq: u32 => "next_seq",
        pub version: u64 => "version",
        pub refs: Vec<RefPath> => "refs" [omitempty],
        pub slots: PageRoot => "slots",
        pub ids: PageRoot => "ids",
        pub indexes: Vec<ManifestIndex> => "indexes" [omitempty],
        pub derived: Vec<ManifestIndex> => "derived_indexes" [omitempty],
        pub origins: BTreeMap<Vec<u8>, Vec<u8>> => "index_origins" [omitempty],
    }
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Manifest {
        pub format: i64 => "format",
        pub checkpoint: Vec<u8> => "checkpoint" [omitempty],
        pub kv: Vec<u8> => "kv" [omitempty],
        pub lsn: u64 => "lsn",
        pub segment: u32 => "segment",
        pub offset: u32 => "offset",
        pub created_at: Vec<u8> => "created_at",
        pub version: Vec<u8> => "elysiandb_version" [omitempty],
        pub types: Vec<Vec<u8>> => "types" [omitempty],
        pub entities: Vec<ManifestEntity> => "entities" [omitempty],
        pub live: BTreeMap<Vec<u8>, i64> => "live" [omitempty],
        pub previous: u64 => "previous_lsn" [omitempty],
    }
}

impl ManifestEntity {
    fn add_index(&mut self, spec: &IndexSpec, mi: ManifestIndex) {
        if spec.derived() {
            self.derived.push(mi);
            return;
        }

        self.indexes.push(mi);
    }
}

impl Manifest {
    pub fn roots(&self) -> Vec<PageRoot> {
        let mut out = Vec::new();
        for me in &self.entities {
            out.push(me.slots.clone());
            out.push(me.ids.clone());
            for mi in me.indexes.iter().chain(me.derived.iter()) {
                if !mi.dormant {
                    out.push(mi.root.clone());
                }
            }
        }

        out
    }

    pub fn encode(&self) -> Result<Vec<u8>, json::Error> {
        let mut data = json::marshal_indent(self, "", "  ")?;
        data.push(b'\n');
        Ok(data)
    }
}

pub fn parse_manifest(data: &[u8]) -> Result<Manifest, BoxError> {
    let mut m = Manifest::default();
    match json::unmarshal(data, &mut m) {
        Ok(()) => {}
        Err(err) => {
            return Err(errors::wrap(
                &format!("cannot read {MANIFEST_FILE}: "),
                Box::new(err),
            ));
        }
    }

    if m.format != MANIFEST_FORMAT && m.format != MANIFEST_FORMAT_PAGES {
        return Err(errors::new(format!(
            "{MANIFEST_FILE} has format {}, this version reads formats {MANIFEST_FORMAT} and {MANIFEST_FORMAT_PAGES}",
            m.format
        )));
    }

    Ok(m)
}

fn json_string(b: &[u8]) -> String {
    match json::marshal(&b.to_vec()) {
        Ok(v) => String::from_utf8_lossy(&v).into_owned(),
        Err(_) => String::new(),
    }
}

pub(crate) type Chosen = (Option<Arc<Manifest>>, Option<Arc<Manifest>>);

pub(crate) struct CkptState {
    pub(crate) manifests: Manifests,
    pub(crate) current: Option<Arc<Manifest>>,
    pub(crate) previous: Option<Arc<Manifest>>,
    pub(crate) page_removal: Vec<u32>,
    pub(crate) last_page_walk: Option<Instant>,
}

impl CkptState {
    pub(crate) fn new(manifests: Manifests) -> CkptState {
        CkptState {
            manifests,
            current: None,
            previous: None,
            page_removal: Vec::new(),
            last_page_walk: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CheckpointState {
    pub(crate) pos: Position,
    pub(crate) at: Option<Instant>,
}

struct EntitySnapshot {
    ent: Arc<Entity>,
    name: Vec<u8>,
    r: Arc<Root>,
    lag: Option<BTreeSet<Vec<u8>>>,
}

impl EntitySnapshot {
    fn lags(&self, name: &[u8]) -> bool {
        self.lag.as_ref().is_some_and(|l| l.contains(name))
    }
}

pub(crate) fn root_of(t: &Tree) -> PageRoot {
    match t.root() {
        Some(r) => PageRoot {
            id: r.id(),
            size: r.size(),
        },
        None => PageRoot::default(),
    }
}

struct CkReader<'a> {
    data: &'a [u8],
    pos: usize,
    err: bool,
}

impl CkReader<'_> {
    fn uvarint(&mut self) -> u64 {
        if self.err {
            return 0;
        }

        let (v, n) = binary::uvarint(&self.data[self.pos..]);
        if n <= 0 {
            self.err = true;
            return 0;
        }

        self.pos += n as usize;
        v
    }

    fn varint(&mut self) -> i64 {
        if self.err {
            return 0;
        }

        let (v, n) = binary::varint(&self.data[self.pos..]);
        if n <= 0 {
            self.err = true;
            return 0;
        }

        self.pos += n as usize;
        v
    }

    fn str(&mut self) -> Vec<u8> {
        let l = self.uvarint();
        if self.err {
            return Vec::new();
        }

        if ((self.data.len() - self.pos) as u64) < l {
            self.err = true;
            return Vec::new();
        }

        let s = self.data[self.pos..self.pos + l as usize].to_vec();
        self.pos += l as usize;
        s
    }

    fn loc(&mut self) -> Location {
        let segment = self.uvarint() as u32;
        let offset = self.uvarint() as u32;
        let length = self.uvarint() as u32;
        Location {
            segment,
            offset,
            length,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CkEntity {
    pub name: Vec<u8>,
    pub gen_: u64,
    pub next_seq: u32,
    pub version: u64,
    pub refs: Vec<RefPath>,
    pub slots: Vec<Slot>,
    pub indexes: Vec<CkIndex>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CkExtras {
    pub dormant: Vec<IndexSpec>,
    pub search: bool,
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CkIndex {
    pub spec: IndexSpec,
    pub keys: Vec<Vec<u8>>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CkKv {
    pub key: Vec<u8>,
    pub lsn: u64,
    pub expires_at: i64,
    pub loc: Location,
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckpointData {
    pub last_lsn: u64,
    pub pos: Position,
    pub types: Vec<Vec<u8>>,
    pub entities: Vec<CkEntity>,
    pub kv: Vec<CkKv>,
    pub extras: Option<BTreeMap<Vec<u8>, CkExtras>>,
}

#[derive(Debug)]
pub struct UnsupportedCheckpoint(pub u64);

impl fmt::Display for UnsupportedCheckpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "checkpoint version {} is not supported", self.0)
    }
}

impl std::error::Error for UnsupportedCheckpoint {}

fn read_os_file(path: &Path) -> Result<Vec<u8>, BoxError> {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(err) => return Err(Box::new(FsError::new("open", path, err))),
    };

    let mut data = Vec::new();
    if let Err(err) = f.read_to_end(&mut data) {
        return Err(Box::new(FsError::new("read", path, err)));
    }

    Ok(data)
}

#[doc(hidden)]
pub fn read_checkpoint(path: &Path) -> Result<CheckpointData, BoxError> {
    let data = read_os_file(path)?;
    decode_checkpoint(&data)
}

#[doc(hidden)]
pub fn decode_checkpoint(data: &[u8]) -> Result<CheckpointData, BoxError> {
    if data.len() < CHECKPOINT_MAGIC.len() + 4
        || &data[..CHECKPOINT_MAGIC.len()] != CHECKPOINT_MAGIC
    {
        return Err(Box::new(CheckpointCorrupt));
    }

    let body = &data[..data.len() - 4];
    let t = &data[data.len() - 4..];
    if crc32c::checksum(body) != u32::from_le_bytes([t[0], t[1], t[2], t[3]]) {
        return Err(Box::new(CheckpointCorrupt));
    }

    let mut r = CkReader {
        data: body,
        pos: CHECKPOINT_MAGIC.len(),
        err: false,
    };
    let v = r.uvarint();
    if v != CHECKPOINT_VERSION {
        return Err(Box::new(UnsupportedCheckpoint(v)));
    }

    let mut ck = CheckpointData {
        last_lsn: r.uvarint(),
        ..CheckpointData::default()
    };
    let segment = r.uvarint() as u32;
    let offset = r.uvarint() as u32;
    ck.pos = Position { segment, offset };

    let mut n = r.uvarint();
    while n > 0 && !r.err {
        let t = r.str();
        ck.types.push(t);
        n -= 1;
    }

    let mut n = r.uvarint();
    while n > 0 && !r.err {
        let name = r.str();
        let gen_ = r.uvarint();
        let next_seq = r.uvarint() as u32;
        let version = r.uvarint();
        let mut ent = CkEntity {
            name,
            gen_,
            next_seq,
            version,
            ..CkEntity::default()
        };

        let mut m = r.uvarint();
        while m > 0 && !r.err {
            let path = r.str();
            let target = r.str();
            ent.refs.push(RefPath { path, target });
            m -= 1;
        }

        let count = r.uvarint();
        ent.slots.reserve(count.min(1 << 20) as usize);
        let mut m = count;
        while m > 0 && !r.err {
            let seq = r.uvarint() as u32;
            let id = r.str();
            let lsn = r.uvarint();
            let loc = r.loc();
            ent.slots.push(Slot { seq, id, lsn, loc });
            m -= 1;
        }

        let mut m = r.uvarint();
        while m > 0 && !r.err {
            let name = r.str();
            let spec = parse_index_name(&name);
            if spec.is_none() && !r.err {
                r.err = true;
            }

            let mut idx = CkIndex {
                spec: spec.unwrap_or_default(),
                keys: Vec::new(),
            };
            let entries = r.uvarint();
            idx.keys.reserve(entries.min(1 << 20) as usize);
            let mut k = entries;
            while k > 0 && !r.err {
                let key = r.str();
                idx.keys.push(key);
                k -= 1;
            }
            ent.indexes.push(idx);
            m -= 1;
        }

        ck.entities.push(ent);
        n -= 1;
    }

    let mut n = r.uvarint();
    while n > 0 && !r.err {
        let key = r.str();
        let lsn = r.uvarint();
        let expires_at = r.varint();
        let loc = r.loc();
        ck.kv.push(CkKv {
            key,
            lsn,
            expires_at,
            loc,
        });
        n -= 1;
    }

    if !r.err && r.pos < r.data.len() && r.uvarint() == 1 {
        let mut extras = BTreeMap::new();
        let mut n = r.uvarint();
        while n > 0 && !r.err {
            let name = r.str();
            let mut x = CkExtras::default();
            let mut m = r.uvarint();
            while m > 0 && !r.err {
                if let Some(spec) = parse_index_name(&r.str()) {
                    x.dormant.push(spec);
                }
                m -= 1;
            }
            x.search = r.uvarint() == 1;
            extras.insert(name, x);
            n -= 1;
        }
        ck.extras = Some(extras);
    }

    if r.err {
        return Err(Box::new(CheckpointCorrupt));
    }

    Ok(ck)
}

#[derive(Debug)]
pub struct ShortSegment(pub u32);

impl fmt::Display for ShortSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "segment {} is shorter than a value it holds", self.0)
    }
}

impl std::error::Error for ShortSegment {}

struct SegmentCache {
    dir: std::path::PathBuf,
    fs: Arc<dyn Fs>,
    id: u32,
    data: Option<Vec<u8>>,
}

impl SegmentCache {
    fn value(&mut self, loc: Location) -> Result<Vec<u8>, BoxError> {
        if loc.length == 0 {
            return Ok(Vec::new());
        }

        if self.data.is_none() || self.id != loc.segment {
            let path = filepath::join(&self.dir, crate::journal::segment_name(loc.segment));
            let data = durable::read_file(&*self.fs, &path)?;
            self.id = loc.segment;
            self.data = Some(data);
        }

        let data = self.data.as_deref().unwrap_or_default();
        let end = loc.offset as usize + loc.length as usize;
        if end > data.len() {
            return Err(Box::new(ShortSegment(loc.segment)));
        }

        Ok(data[loc.offset as usize..end].to_vec())
    }
}

impl Engine {
    pub(crate) fn check_manifest(&self, m: &Manifest) -> Result<(), BoxError> {
        if m.format != MANIFEST_FORMAT_PAGES {
            return Ok(());
        }

        if !m.kv.is_empty() {
            let path = filepath::join(&self.dir_path(), os_name(&m.kv));
            if let Err(err) = durable::read_kv_file(&*self.files(), &path) {
                return Err(errors::wrap(
                    &format!("cannot read {}: ", String::from_utf8_lossy(&m.kv)),
                    Box::new(err),
                ));
            }
        }

        for r in m.roots() {
            if r.id == 0 {
                continue;
            }
            if let Err(err) = self.pages.load(&Ref::stored(r.id, r.size)) {
                return Err(Box::new(err));
            }
        }

        Ok(())
    }

    pub(crate) fn choose_manifest(&self) -> Result<Chosen, BoxError> {
        let stored = self.ckpt.lock().manifests.stored()?;

        let mut chosen: Option<Arc<Manifest>> = None;
        let mut previous: Option<Arc<Manifest>> = None;
        let mut reasons: Vec<String> = Vec::new();
        for (i, sm) in stored.iter().enumerate() {
            let mut parsed = parse_manifest(&sm.data);
            if chosen.is_none()
                && let Ok(m) = &parsed
                && let Err(err) = self.check_manifest(m)
            {
                parsed = Err(err);
            }

            let m = match parsed {
                Ok(m) => m,
                Err(err) => {
                    reasons.push(format!("{}: {}", sm.name, err));
                    continue;
                }
            };

            match &chosen {
                None => {
                    chosen = Some(Arc::new(m));
                    self.ckpt.lock().manifests.adopt(&sm.data);
                    if i > 0 {
                        ENGINE_LOG.warn(
                            "the last checkpoint is unreadable, the previous one is used",
                            crate::kv!["reason" => reasons[0].as_str()],
                        );
                    }
                }
                Some(c) => {
                    if c.previous != 0 && c.previous == m.lsn && m.format == MANIFEST_FORMAT_PAGES {
                        previous = Some(Arc::new(m));
                    }
                }
            }
        }

        if chosen.is_none() && !stored.is_empty() {
            return Err(errors::new(format!(
                "no readable checkpoint: {}",
                reasons.join("; ")
            )));
        }

        Ok((chosen, previous))
    }

    pub(crate) fn open_tree(&self, r: &PageRoot) -> Tree {
        if r.id == 0 {
            return self.new_tree();
        }

        Tree::open(Some(self.loader.clone()), Some(Ref::stored(r.id, r.size)))
    }

    pub(crate) fn restore(&self, ck: &CheckpointData) -> Result<(), BoxError> {
        self.catalog.write().types = ck.types.clone();

        let mut live: BTreeMap<u32, i64> = BTreeMap::new();
        for ce in &ck.entities {
            let ent = self.entity_or_create(&ce.name);
            let _g = ent.apply.lock();

            let mut by_seq: Vec<&Slot> = ce.slots.iter().collect();
            crate::gocompat::sort::slice(&mut by_seq, |a, b| a.seq < b.seq);
            let mut seq_keys: Vec<[u8; 4]> = Vec::with_capacity(by_seq.len());
            let mut seq_vals: Vec<Vec<u8>> = Vec::with_capacity(by_seq.len());
            for sl in &by_seq {
                seq_keys.push(seq_key(sl.seq));
                let mut v = Vec::new();
                encode_slot(&mut v, sl);
                seq_vals.push(v);
                *live.entry(sl.loc.segment).or_default() += i64::from(sl.loc.length);
            }

            let mut by_id: Vec<&Slot> = ce.slots.iter().collect();
            crate::gocompat::sort::slice(&mut by_id, |a, b| a.id < b.id);
            let id_keys: Vec<&[u8]> = by_id.iter().map(|s| s.id.as_slice()).collect();
            let id_vals: Vec<[u8; 4]> = by_id.iter().map(|s| seq_key(s.seq)).collect();

            let mut indexes: Indexes = BTreeMap::new();
            for ci in &ce.indexes {
                indexes.insert(
                    ci.spec.name(),
                    Index::ready(
                        ci.spec.clone(),
                        Tree::from_sorted(Some(self.loader.clone()), &ci.keys, None::<&[Vec<u8>]>),
                    ),
                );
            }

            if let Some(extras) = &ck.extras
                && let Some(x) = extras.get(&ce.name)
            {
                for spec in &x.dormant {
                    indexes
                        .entry(spec.name())
                        .or_insert_with(|| Index::dormant(spec.clone()));
                }
            }

            let r = Root {
                gen_: ce.gen_,
                next_seq: ce.next_seq,
                slots: Tree::from_sorted(Some(self.loader.clone()), &seq_keys, Some(&seq_vals)),
                ids: Tree::from_sorted(Some(self.loader.clone()), &id_keys, Some(&id_vals)),
                indexes: Arc::new(indexes),
                version: ce.version,
                refs: Arc::new(ce.refs.clone()),
                lagging: false,
            };

            ent.root.store(Arc::new(r));
        }

        self.live.reset(&live);

        self.restore_kv(
            ck.kv
                .iter()
                .map(|k| (k.key.clone(), k.lsn, k.expires_at, k.loc))
                .collect(),
        )
    }

    fn restore_kv(&self, entries: Vec<(Vec<u8>, u64, i64, Location)>) -> Result<(), BoxError> {
        let mut loads = entries;
        crate::gocompat::sort::slice(&mut loads, |a, b| {
            if a.3.segment != b.3.segment {
                return a.3.segment < b.3.segment;
            }
            a.3.offset < b.3.offset
        });

        let mut sc = SegmentCache {
            dir: self.journal().dir().to_path_buf(),
            fs: self.files(),
            id: 0,
            data: None,
        };
        for (key, lsn, expires_at, loc) in loads {
            let v = sc.value(loc)?;
            let mut sh = self.kv.shard(&key).write();
            self.kv.set_locked(
                &mut sh,
                &key,
                KvEntry {
                    value: Some(v),
                    expires_at,
                    lsn,
                    loc,
                },
            );
        }

        Ok(())
    }

    pub(crate) fn restore_pages(&self, m: &Manifest) -> Result<(), BoxError> {
        self.catalog.write().types = m.types.clone();

        for me in &m.entities {
            let ent = self.entity_or_create(&me.name);
            let _g = ent.apply.lock();

            let mut indexes: Indexes = BTreeMap::new();
            for (name, origin) in &me.origins {
                ent.meta(name).set_origin(origin);
            }

            for mi in me.indexes.iter().chain(me.derived.iter()) {
                let Some(spec) = parse_index_name(&mi.name) else {
                    return Err(errors::new(format!(
                        "{MANIFEST_FILE} names an unknown index {}",
                        json_string(&mi.name)
                    )));
                };

                if mi.dormant {
                    indexes.insert(mi.name.clone(), Index::dormant(spec));
                    continue;
                }

                indexes.insert(
                    mi.name.clone(),
                    Index::ready(spec, self.open_tree(&mi.root)),
                );
            }

            let r = Root {
                gen_: me.gen_,
                next_seq: me.next_seq,
                slots: self.open_tree(&me.slots),
                ids: self.open_tree(&me.ids),
                indexes: Arc::new(indexes),
                version: me.version,
                refs: Arc::new(me.refs.clone()),
                lagging: false,
            };

            ent.root.store(Arc::new(r));
        }

        let mut live: BTreeMap<u32, i64> = BTreeMap::new();
        for (seg, &n) in &m.live {
            let Ok(id) = strconv::parse_uint(seg, 10, 32) else {
                return Err(errors::new(format!(
                    "{MANIFEST_FILE} has an invalid segment {}",
                    json_string(seg)
                )));
            };
            live.insert(id as u32, n);
        }
        self.live.reset(&live);

        if m.kv.is_empty() {
            return Ok(());
        }

        let path = filepath::join(&self.dir_path(), os_name(&m.kv));
        let records = match durable::read_kv_file(&*self.files(), &path) {
            Ok(r) => r,
            Err(err) => {
                return Err(errors::wrap(
                    &format!(
                        "cannot read the checkpoint {}: ",
                        String::from_utf8_lossy(&m.kv)
                    ),
                    Box::new(err),
                ));
            }
        };

        self.restore_kv(
            records
                .into_iter()
                .map(|r| {
                    (
                        r.key,
                        r.lsn,
                        r.expires_at,
                        Location {
                            segment: r.segment,
                            offset: r.offset,
                            length: r.length,
                        },
                    )
                })
                .collect(),
        )
    }

    pub(crate) fn finish_upgrade(&self) -> Result<(), BoxError> {
        if let Err(err) = self.checkpoint_with(true) {
            return Err(errors::wrap("cannot write the upgraded checkpoint: ", err));
        }

        let files = self.files();
        let entries = files.read_dir(&self.dir_path())?;

        let legacy = filepath::join(&self.dir_path(), LEGACY_CHECKPOINTS);
        let mut moved = 0usize;
        for entry in entries {
            let name = entry.name().as_encoded_bytes();
            if !name.starts_with(b"checkpoint-") || !name.ends_with(b".ckpt") {
                continue;
            }

            files.mkdir_all(&legacy, 0o755)?;
            files.rename(
                &filepath::join(&self.dir_path(), entry.name()),
                &filepath::join(&legacy, entry.name()),
            )?;
            moved += 1;
        }

        let _ = files.sync_dir(&self.dir_path());
        ENGINE_LOG.info(
            "storage format upgraded",
            crate::kv![
                "from" => self.upgrade_from.load(Ordering::SeqCst),
                "to" => MANIFEST_FORMAT_PAGES,
                "kept_in" => LEGACY_CHECKPOINTS,
                "files" => moved,
            ],
        );
        self.upgrade_from.store(0, Ordering::SeqCst);

        Ok(())
    }

    pub(crate) fn last_checkpoint(&self) -> CheckpointState {
        match self.ck_state.load_full() {
            Some(st) => *st,
            None => CheckpointState::default(),
        }
    }

    pub fn checkpoint(&self) -> Result<(), BoxError> {
        self.checkpoint_with(false)
    }

    pub(crate) fn checkpoint_with(&self, force: bool) -> Result<(), BoxError> {
        let mut ck = self.ckpt.lock();

        let start = Instant::now();
        let pos = self.journal().position();
        let lsn = self.journal().next_lsn().wrapping_sub(1);
        let last = self.last_checkpoint();
        if !force && pos == last.pos && last.at.is_some() {
            return Ok(());
        }

        let ents = self.all_entities();
        let mut snaps: Vec<EntitySnapshot> = Vec::with_capacity(ents.len());
        for ent in ents {
            let g = ent.apply.lock();
            let r = ent.root.load_full();
            let lag = ent.backlog.lag_names();
            drop(g);
            snaps.push(EntitySnapshot {
                name: ent.name.clone(),
                ent,
                r,
                lag,
            });
        }

        let types = {
            let _g = self.types_mu.lock();
            self.types()
        };

        let mut kv: Vec<(Vec<u8>, Arc<KvEntry>)> = Vec::new();
        for sh in &self.kv.shards {
            let g = sh.read();
            for (k, entry) in g.iter() {
                kv.push((k.clone(), entry.clone()));
            }
        }
        kv.sort_by(|a, b| a.0.cmp(&b.0));

        let doc_live = self.live.snapshot();

        if let Err(err) = self.journal().flush() {
            return Err(errors::wrap(
                "cannot flush the log before a checkpoint: ",
                err,
            ));
        }

        let mut m = Manifest {
            format: MANIFEST_FORMAT_PAGES,
            lsn,
            segment: pos.segment,
            offset: pos.offset,
            types,
            ..Manifest::default()
        };
        for snap in &snaps {
            let mut trees: Vec<Tree> = vec![snap.r.slots.clone(), snap.r.ids.clone()];
            let mut me = ManifestEntity {
                name: snap.name.clone(),
                gen_: snap.r.gen_,
                next_seq: snap.r.next_seq,
                version: snap.r.version,
                refs: (*snap.r.refs).clone(),
                ..ManifestEntity::default()
            };

            for (name, idx) in snap.r.indexes.iter() {
                let origin = snap.ent.meta(name).origin();
                if origin != ORIGIN_QUERY.as_bytes() {
                    me.origins.insert(name.clone(), origin);
                }
                if idx.dormant {
                    me.add_index(
                        &idx.spec,
                        ManifestIndex {
                            name: name.clone(),
                            dormant: true,
                            ..ManifestIndex::default()
                        },
                    );
                } else if idx.ready && snap.lags(name) {
                    me.add_index(
                        &idx.spec,
                        ManifestIndex {
                            name: name.clone(),
                            dormant: true,
                            ..ManifestIndex::default()
                        },
                    );
                    trees.push(idx.tree.clone());
                } else if idx.ready {
                    trees.push(idx.tree.clone());
                }
            }

            let mut flush_err: Option<BoxError> = None;
            durable::run_gently(|| {
                for t in &trees {
                    if let Err(err) = self.pages.flush_tree(t) {
                        flush_err = Some(err);
                        return;
                    }
                }
            });
            if let Some(err) = flush_err {
                return Err(errors::wrap(
                    &format!(
                        "cannot write the pages of {}: ",
                        String::from_utf8_lossy(&snap.name)
                    ),
                    err,
                ));
            }

            me.slots = root_of(&snap.r.slots);
            me.ids = root_of(&snap.r.ids);
            for (name, idx) in snap.r.indexes.iter() {
                if idx.ready && !snap.lags(name) {
                    me.add_index(
                        &idx.spec,
                        ManifestIndex {
                            name: name.clone(),
                            root: root_of(&idx.tree),
                            dormant: false,
                        },
                    );
                }
            }
            me.indexes.sort_by(|a, b| a.name.cmp(&b.name));
            me.derived.sort_by(|a, b| a.name.cmp(&b.name));

            m.entities.push(me);
        }

        let mut sync_err: Option<FsError> = None;
        durable::run_gently(|| {
            if let Err(err) = self.pages.sync() {
                sync_err = Some(err);
            }
        });
        if let Some(err) = sync_err {
            return Err(errors::wrap("cannot sync the pages: ", Box::new(err)));
        }

        let mut live: BTreeMap<u32, i64> = doc_live.clone();

        m.kv = format!("kv-{lsn:016}.ckpt").into_bytes();
        let mut records: Vec<KvRecord> = Vec::with_capacity(kv.len());
        for (key, entry) in &kv {
            let loc = entry.loc;
            records.push(KvRecord {
                key: key.clone(),
                lsn: entry.lsn,
                expires_at: entry.expires_at,
                segment: loc.segment,
                offset: loc.offset,
                length: loc.length,
            });
            *live.entry(loc.segment).or_default() += i64::from(loc.length);
        }

        let kv_path = filepath::join(&self.dir_path(), os_name(&m.kv));
        if let Err(err) = durable::write_kv_file(&*self.files(), &kv_path, &records) {
            return Err(errors::wrap("cannot write the checkpoint: ", Box::new(err)));
        }

        for (seg, n) in &doc_live {
            m.live
                .insert(strconv::format_uint(u64::from(*seg), 10).into_bytes(), *n);
        }

        m.created_at = Time::now().utc().format(gotime::RFC3339).into_bytes();
        if let Some(cur) = &ck.current {
            m.previous = cur.lsn;
        }
        let data = m.encode()?;

        if let Err(err) = ck.manifests.commit(&data) {
            return Err(errors::wrap("cannot write the manifest: ", Box::new(err)));
        }

        ck.previous = ck.current.take();
        ck.current = Some(Arc::new(m));
        self.remove_old_checkpoints(&ck);
        self.ck_state.store(Some(Arc::new(CheckpointState {
            pos,
            at: Some(Instant::now()),
        })));
        self.segment_live.store(Some(Arc::new(live.clone())));
        self.stats.checkpoints.fetch_add(1, Ordering::SeqCst);
        self.stats
            .checkpoint_nanos
            .store(start.elapsed().as_nanos() as i64, Ordering::SeqCst);

        let mut retain = pos;
        if let Some(prev) = &ck.previous
            && prev.format == MANIFEST_FORMAT_PAGES
        {
            retain = Position {
                segment: prev.segment,
                offset: prev.offset,
            };
        }
        self.retire_dead_segments(retain, &live);
        self.guarded("page cleaning", || self.clean_pages(&snaps, &mut ck));
        self.pages.sweep();

        ENGINE_LOG.debug(
            "checkpoint written",
            crate::kv![
                "lsn" => lsn,
                "entities" => snaps.len(),
                "kv_keys" => kv.len(),
                "duration_ms" => log::since(start),
            ],
        );

        Ok(())
    }

    fn remove_old_checkpoints(&self, ck: &CkptState) {
        let mut keep: BTreeSet<Vec<u8>> = BTreeSet::new();
        for m in [&ck.current, &ck.previous].into_iter().flatten() {
            if !m.kv.is_empty() {
                keep.insert(m.kv.clone());
            }
        }

        let files = self.files();
        let Ok(entries) = files.read_dir(&self.dir_path()) else {
            return;
        };

        for entry in entries {
            let name = entry.name().as_encoded_bytes();
            if !keep.contains(name)
                && name.starts_with(b"kv-")
                && (name.ends_with(b".ckpt") || name.ends_with(b".ckpt.tmp"))
            {
                let _ = files.remove(&filepath::join(&self.dir_path(), entry.name()));
            }
        }
    }

    pub(crate) fn reclaim_due(&self) -> bool {
        if self.reclaim.load(Ordering::SeqCst) <= 0
            || self.dirty.load(Ordering::SeqCst) >= self.dirty_flush_bytes() / 8
            || self.log_since_checkpoint() >= RECLAIM_QUIET_LOG
        {
            return false;
        }

        match *self.last_reclaim.lock() {
            None => true,
            Some(t) => t.elapsed() >= super::compact::RECLAIM_SPACING,
        }
    }

    pub(crate) fn reclaim_round(&self) -> Result<(), BoxError> {
        *self.last_reclaim.lock() = Some(Instant::now());
        self.ckpt.lock().last_page_walk = None;

        let left = self.reclaim.fetch_sub(1, Ordering::SeqCst) - 1;
        self.pages.rotate()?;

        self.checkpoint_with(true)?;

        if left <= 0 && self.reclaim.load(Ordering::SeqCst) <= 0 {
            return_memory(RETURN_MEMORY_ABOVE);
        }

        Ok(())
    }

    pub fn compact_pages(&self) -> Result<usize, BoxError> {
        let before = self.pages.segment_sizes().len();
        for _ in 0..COMPACT_PAGES_ROUNDS {
            self.ckpt.lock().last_page_walk = None;

            self.checkpoint_with(true)?;
        }

        Ok(before.saturating_sub(self.pages.segment_sizes().len()))
    }

    pub fn page_stats(&self) -> durable::PageStats {
        self.pages.stats()
    }

    fn clean_pages(&self, snaps: &[EntitySnapshot], ck: &mut MutexGuard<'_, CkptState>) {
        if ck.page_removal.is_empty()
            && ck
                .last_page_walk
                .is_some_and(|t| t.elapsed() < PAGE_WALK_EVERY)
        {
            return;
        }
        ck.last_page_walk = Some(Instant::now());

        let mut before: BTreeMap<u32, i64> = BTreeMap::new();
        if let Some(prev) = &ck.previous
            && prev.format == MANIFEST_FORMAT_PAGES
        {
            for r in prev.roots() {
                self.open_tree(&r).walk_stored(|id, size| {
                    let (seg, _) = split_page_id(id);
                    *before.entry(seg).or_default() += i64::from(size) + durable::PAGE_HEADER_SIZE;
                });
            }
        }

        let mut now: BTreeMap<u32, i64> = BTreeMap::new();
        for snap in snaps {
            let mut trees: Vec<&Tree> = vec![&snap.r.slots, &snap.r.ids];
            for idx in snap.r.indexes.values() {
                if idx.ready {
                    trees.push(&idx.tree);
                }
            }

            for t in trees {
                t.walk_stored(|id, size| {
                    let (seg, _) = split_page_id(id);
                    *now.entry(seg).or_default() += i64::from(size) + durable::PAGE_HEADER_SIZE;
                });
            }
        }

        let current = self.pages.current_segment();
        let mut dead: Vec<u32> = Vec::new();
        let mut candidates: Vec<(u32, f64)> = Vec::new();
        let (mut total, mut held) = (0i64, 0i64);
        let mut leaving = 0usize;
        for (seg, size) in self.pages.segment_sizes() {
            let n = now.get(&seg).copied().unwrap_or(0);
            let b = before.get(&seg).copied().unwrap_or(0);
            total += size;
            held += n;
            if seg >= current {
                continue;
            }

            if n == 0 && b == 0 {
                dead.push(seg);
            } else if n == 0 {
                leaving += 1;
            } else if n * 2 < size {
                candidates.push((seg, n as f64 / size as f64));
            }
        }

        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut limit = PAGE_VICTIMS_PER_CYCLE;
        if self.dirty.load(Ordering::SeqCst) >= self.dirty_flush_bytes() / 2 {
            limit = 1;
        }
        let victims: Vec<u32> = candidates.iter().take(limit).map(|c| c.0).collect();

        let dead_count = dead.len();
        self.pages.remove(&mut dead);
        ck.page_removal = victims.clone();
        self.stats.live_pages.store(held, Ordering::SeqCst);

        let garbage = total - held;
        if garbage >= RECLAIM_ABOVE_BYTES
            && garbage * 4 >= total
            && dead_count + victims.len() + leaving > 0
        {
            let cur = self.reclaim.load(Ordering::SeqCst);
            self.reclaim.store(cur.max(1), Ordering::SeqCst);
        }

        if victims.is_empty() {
            return;
        }

        let set: BTreeSet<u32> = victims.into_iter().collect();
        self.relocate_pages(&|id| set.contains(&split_page_id(id).0));
    }

    fn relocate_pages(&self, victim: &dyn Fn(u64) -> bool) {
        for ent in self.all_entities() {
            let cur = ent.root.load_full();
            cur.slots.preload(victim);
            cur.ids.preload(victim);
            for idx in cur.indexes.values() {
                if idx.ready {
                    idx.tree.preload(victim);
                }
            }

            while self.relocate_batch(&ent, victim) >= RELOCATE_BATCH
                && !self.closed.load(Ordering::SeqCst)
            {}
        }
    }

    fn relocate_batch(&self, ent: &Arc<Entity>, victim: &dyn Fn(u64) -> bool) -> usize {
        let _g = ent.apply.lock();

        let cur = ent.root.load_full();
        let tok = Token::new();

        let mut nr = (*cur).clone();
        let mut moved = 0usize;
        let mut relocate = |t: &Tree| -> Tree {
            if moved >= RELOCATE_BATCH {
                return t.clone();
            }
            let (out, n) = t
                .with_token(Some(&tok))
                .relocate(victim, RELOCATE_BATCH - moved);
            moved += n;
            out.with_token(None)
        };

        nr.slots = relocate(&cur.slots);
        nr.ids = relocate(&cur.ids);
        let mut indexes = (*cur.indexes).clone();
        for (name, idx) in cur.indexes.iter() {
            if idx.ready && !ent.backlog.is_running(name) {
                indexes.insert(
                    name.clone(),
                    Index::ready(idx.spec.clone(), relocate(&idx.tree)),
                );
            }
        }
        nr.indexes = Arc::new(indexes);
        tok.freeze();

        if moved > 0 {
            ent.root.store(Arc::new(nr));
        }

        moved
    }

    fn retire_dead_segments(&self, pos: Position, live: &BTreeMap<u32, i64>) {
        let Ok(segments) = self.journal().segments() else {
            return;
        };

        for seg in segments {
            if seg.id >= pos.segment {
                break;
            }

            if live.get(&seg.id).copied().unwrap_or(0) == 0
                && let Err(err) = self.journal().retire(seg.id)
            {
                ENGINE_LOG.warn(
                    "cannot remove a log segment",
                    crate::kv!["segment" => seg.id, "err" => log::display(&err)],
                );
            }
        }
    }

    pub(crate) fn dir_path(&self) -> std::path::PathBuf {
        self.folder().to_path_buf()
    }
}

pub(crate) fn os_name(b: &[u8]) -> &std::ffi::OsStr {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(b)
}
