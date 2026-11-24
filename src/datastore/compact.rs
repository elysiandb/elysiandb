use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::gocompat::errors::BoxError;
use crate::journal::{self, Position, Record};
use crate::log;

use super::build::{IDLE_SWEEP_INTERVAL, INDEX_IDLE_AFTER};
use super::engine::{ENGINE_LOG, Engine, WRITE_FAILURES, Wake};
use super::entity::Draft;
use super::kv::KvEntry;

pub const COMPACTION_INTERVAL: Duration = Duration::from_secs(10);
pub const COMPACTION_RATIO: f64 = 0.5;
pub const COMPACTION_BATCH: usize = 512;
pub const CHECKPOINT_SPACING: i64 = 10;
pub const CHECKPOINT_MAX_GAP: Duration = Duration::from_secs(5 * 60);
pub const FORCED_CHECKPOINT_BYTES: i64 = 256 << 20;
pub const RECLAIM_ROUNDS: i32 = 3;
pub const RECLAIM_SPACING: Duration = Duration::from_secs(2);
pub const MAX_FLUSH_PAUSE: Duration = Duration::from_millis(250);
pub const MAX_COMPACTION_BATCH: usize = 8;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineStats {
    pub checkpoints: i64,
    pub last_checkpoint_ms: f64,
    pub log_since_checkpoint: i64,
    pub compacted_segments: i64,
    pub compacted_bytes: i64,
    pub conflicts: i64,
    pub value_cache_bytes: i64,
    pub value_cache_hits: i64,
    pub value_cache_misses: i64,
    pub segments: usize,
    pub log_bytes: i64,
    pub live_bytes: i64,
    pub pending_bytes: usize,
    pub dirty_page_bytes: i64,
    pub index_count: usize,
    pub index_bytes: i64,
    pub inline: bool,
    pub documents: i64,
    pub resident_struct_bytes: i64,
    pub page_cache_limit: i64,
    pub page_loads: i64,
    pub page_writes: i64,
    pub live_page_bytes: i64,
}

impl Engine {
    pub(crate) fn maintenance_loop(&self) {
        let mut ticker = crate::sync::Ticker::new(Duration::from_secs(1));
        let mut last_compaction = Instant::now();
        let mut last_sweep = Instant::now();
        let mut compacted = false;
        loop {
            let volume = match self.bell.wait_flush_or(ticker.deadline()) {
                Wake::Stop => return,
                Wake::Flush => true,
                Wake::Tick => {
                    if ticker.take(Instant::now()).is_none() {
                        continue;
                    }
                    false
                }
            };

            self.guarded("maintenance", || {
                if last_sweep.elapsed() >= IDLE_SWEEP_INTERVAL {
                    last_sweep = Instant::now();
                    self.drop_idle_indexes(INDEX_IDLE_AFTER);
                    self.advise();
                    self.journal().close_retired();
                }

                if last_compaction.elapsed() >= COMPACTION_INTERVAL {
                    last_compaction = Instant::now();
                    match self.compact(self.compaction_budget()) {
                        Err(err) => ENGINE_LOG
                            .error("compaction failed", crate::kv!["err" => log::display(&err)]),
                        Ok(n) if n > 0 => compacted = true,
                        Ok(_) => {}
                    }
                }
            });

            self.guarded("checkpoint", || self.maintain(volume, &mut compacted));
        }
    }

    fn compaction_budget(&self) -> usize {
        let Ok(segments) = self.journal().segments() else {
            return 1;
        };

        let Some(live) = self.segment_live.load_full() else {
            return 1;
        };

        let mut reclaimable = 0usize;
        for seg in &segments {
            if seg.size > 0
                && (live.get(&seg.id).copied().unwrap_or(0) as f64) / (seg.size as f64)
                    < COMPACTION_RATIO
            {
                reclaimable += 1;
            }
        }

        (reclaimable / 2).clamp(1, MAX_COMPACTION_BATCH)
    }

    fn maintain(&self, volume: bool, compacted: &mut bool) {
        if !volume
            && self.dirty.load(Ordering::SeqCst) < self.dirty_flush_bytes()
            && self.reclaim_due()
        {
            *compacted = false;
            if let Err(err) = self.reclaim_round() {
                self.log_checkpoint_failure(&err);
            }
            self.measure_dirty();
            self.signal_flushed();
            return;
        }

        let volume = volume || self.dirty.load(Ordering::SeqCst) >= self.dirty_flush_bytes();
        if volume
            || ((*compacted || self.checkpoint_due())
                && (self.checkpoint_spaced()
                    || self.log_since_checkpoint() >= self.forced_checkpoint_log()))
        {
            *compacted = false;
            if let Err(err) = self.checkpoint(volume) {
                self.log_checkpoint_failure(&err);
            }
            self.measure_dirty();
            self.signal_flushed();
        }
    }

    fn log_checkpoint_failure(&self, err: &BoxError) {
        let (suppressed, ok) = WRITE_FAILURES.allow("checkpoint");
        if ok {
            ENGINE_LOG.error(
                "checkpoint failed",
                &log::with_suppressed(crate::kv!["err" => log::display(err)], suppressed),
            );
        }
    }

    pub fn log_since_checkpoint(&self) -> i64 {
        let pos = self.journal().position();
        let mut ck = self.last_checkpoint().pos;

        if ck.segment == 0 {
            ck = Position {
                segment: 1,
                offset: 16,
            };
        }

        if pos.segment == ck.segment {
            return i64::from(pos.offset) - i64::from(ck.offset);
        }

        let mut size = self.opts.segment_size;
        if size <= 0 {
            size = journal::DEFAULT_SEGMENT_SIZE;
        }

        i64::from(pos.segment.wrapping_sub(ck.segment).wrapping_sub(1)) * size
            + (size - i64::from(ck.offset))
            + i64::from(pos.offset)
    }

    fn forced_checkpoint_log(&self) -> i64 {
        (2 * self.opts.max_log_bytes).max(FORCED_CHECKPOINT_BYTES)
    }

    pub(crate) fn flush_pause(&self, synced: Duration) -> Duration {
        if self.closed.load(Ordering::SeqCst)
            || self.dirty.load(Ordering::SeqCst) >= self.dirty_flush_bytes() * 3 / 2
        {
            return Duration::ZERO;
        }

        synced.min(MAX_FLUSH_PAUSE)
    }

    fn checkpoint_spaced(&self) -> bool {
        let Some(at) = self.last_checkpoint().at else {
            return true;
        };

        let nanos = self.stats.checkpoint_nanos.load(Ordering::SeqCst);
        let spacing = Duration::from_nanos(nanos.saturating_mul(CHECKPOINT_SPACING).max(0) as u64);
        let gap = CHECKPOINT_MAX_GAP.min(spacing);

        at.elapsed() >= gap
    }

    fn checkpoint_due(&self) -> bool {
        let since = self.log_since_checkpoint();
        if since <= 0 {
            return false;
        }

        if self.opts.max_log_bytes > 0 && since >= self.opts.max_log_bytes {
            return true;
        }

        if self.opts.checkpoint_every.is_zero() {
            return false;
        }

        match self.last_checkpoint().at {
            None => true,
            Some(at) => at.elapsed() >= self.opts.checkpoint_every,
        }
    }

    pub fn compact(&self, max_segments: usize) -> Result<usize, BoxError> {
        let Some(live) = self.segment_live.load_full() else {
            return Ok(0);
        };

        let segments = self.journal().segments()?;

        let ckpt = self.last_checkpoint().pos;

        let mut done = 0usize;
        for seg in segments {
            if done >= max_segments {
                break;
            }

            if seg.id >= ckpt.segment || seg.size <= 0 {
                continue;
            }

            let l = live.get(&seg.id).copied().unwrap_or(0);
            let ratio = l as f64 / seg.size as f64;
            if l == 0 || ratio >= COMPACTION_RATIO {
                continue;
            }

            let start = Instant::now();
            let moved = self.compact_segment(seg.id)?;

            done += 1;
            self.stats.compacted_segments.fetch_add(1, Ordering::SeqCst);
            self.stats
                .compacted_bytes
                .fetch_add(seg.size, Ordering::SeqCst);
            ENGINE_LOG.debug(
                "log segment compacted",
                crate::kv![
                    "segment" => seg.id,
                    "moved" => moved,
                    "bytes" => seg.size,
                    "duration_ms" => log::since(start),
                ],
            );
        }

        Ok(done)
    }

    fn compact_segment(&self, id: u32) -> Result<usize, BoxError> {
        let mut moved = 0usize;
        self.journal().scan_segment(id, |recs| {
            let mut docs: BTreeMap<Vec<u8>, Vec<&Record>> = BTreeMap::new();
            let mut kvs: Vec<&Record> = Vec::new();
            for rec in recs.iter() {
                match rec.kind {
                    journal::KIND_DOC_PUT | journal::KIND_DOC_MOVE => {
                        docs.entry(rec.entity.clone()).or_default().push(rec);
                    }
                    journal::KIND_KV_PUT | journal::KIND_KV_MOVE => kvs.push(rec),
                    _ => {}
                }
            }

            for (name, list) in &docs {
                moved += self.move_docs(name, list)?;
            }

            for rec in kvs {
                if self.move_kv(rec)? {
                    moved += 1;
                }
            }

            Ok(())
        })?;

        Ok(moved)
    }

    fn move_docs(&self, name: &[u8], recs: &[&Record]) -> Result<usize, BoxError> {
        let Some(ent) = self.entity(name, false) else {
            return Ok(0);
        };

        let mut moved = 0usize;
        let mut start = 0usize;
        while start < recs.len() {
            let end = (start + COMPACTION_BATCH).min(recs.len());

            let mut guard = ent.apply.lock();
            let r = ent.root.load_full();
            let mut batch: Vec<Record> = Vec::new();
            for rec in &recs[start..end] {
                let Some(s) = r.slot_by_id(&rec.key) else {
                    continue;
                };
                if s.lsn != rec.lsn || rec.r#gen != r.gen_ {
                    continue;
                }

                batch.push(Record {
                    kind: journal::KIND_DOC_MOVE,
                    entity: name.to_vec(),
                    key: rec.key.clone(),
                    r#gen: r.gen_,
                    value: rec.value.clone(),
                    ..Record::default()
                });
            }

            if batch.is_empty() {
                drop(guard);
                start += COMPACTION_BATCH;
                continue;
            }

            if let Err(err) = self.journal().append(&mut batch) {
                drop(guard);
                return Err(Box::new(err));
            }

            {
                let mut d = Draft::new(self, &ent, &mut guard);
                for rec in &batch {
                    d.put(rec, None, false);
                }
                let released = d.publish();
                if !released.is_empty() {
                    self.release_values(released);
                }
            }
            drop(guard);

            moved += batch.len();
            start += COMPACTION_BATCH;
        }

        Ok(moved)
    }

    fn move_kv(&self, rec: &Record) -> Result<bool, BoxError> {
        let mut sh = self.kv.shard(&rec.key).write();

        let Some(entry) = sh.get(&rec.key).cloned() else {
            return Ok(false);
        };
        if entry.lsn != rec.lsn {
            return Ok(false);
        }

        let mut recs = vec![Record {
            kind: journal::KIND_KV_MOVE,
            key: rec.key.clone(),
            value: entry.value.clone().unwrap_or_default(),
            expires_at: entry.expires_at,
            ..Record::default()
        }];
        if let Err(err) = self.journal().append(&mut recs) {
            return Err(Box::new(err));
        }

        let mut moved: KvEntry = (*entry).clone();
        moved.lsn = recs[0].lsn;
        moved.loc = recs[0].loc;
        sh.insert(rec.key.clone(), Arc::new(moved));

        Ok(true)
    }

    pub fn stats(&self) -> EngineStats {
        let mut s = EngineStats {
            checkpoints: self.stats.checkpoints.load(Ordering::SeqCst),
            last_checkpoint_ms: self.stats.checkpoint_nanos.load(Ordering::SeqCst) as f64 / 1e6,
            log_since_checkpoint: self.log_since_checkpoint(),
            compacted_segments: self.stats.compacted_segments.load(Ordering::SeqCst),
            compacted_bytes: self.stats.compacted_bytes.load(Ordering::SeqCst),
            conflicts: self.stats.conflicts.load(Ordering::SeqCst),
            value_cache_hits: self.cache.hits.load(Ordering::SeqCst),
            value_cache_misses: self.cache.misses.load(Ordering::SeqCst),
            pending_bytes: self.journal().pending_bytes(),
            dirty_page_bytes: self.dirty.load(Ordering::SeqCst),
            inline: self.inline,
            live_page_bytes: self.stats.live_pages.load(Ordering::SeqCst),
            ..EngineStats::default()
        };

        if !self.inline {
            s.value_cache_bytes = self.cache.size();
        }

        if let Ok(segs) = self.journal().segments() {
            s.segments = segs.len();
            for seg in segs {
                s.log_bytes += seg.size;
            }
        }

        if let Some(live) = self.segment_live.load_full() {
            for b in live.values() {
                s.live_bytes += b;
            }
        }

        {
            let c = self.catalog.read();
            for ent in c.entities.values() {
                let r = ent.root.load_full();
                s.documents += r.slots.len() as i64;
                for (name, idx) in r.indexes.iter() {
                    if idx.ready && !idx.dormant {
                        s.index_count += 1;
                        s.index_bytes += ent.meta(name).bytes.load(Ordering::SeqCst);
                    }
                }
            }
        }

        let pages = self.pages.stats();
        s.resident_struct_bytes = pages.resident;
        s.page_cache_limit = pages.limit;
        s.page_loads = pages.loads;
        s.page_writes = pages.writes;

        s
    }
}
