package datastore

import (
	"time"

	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/log"
)

const (
	compactionInterval = 10 * time.Second
	compactionRatio    = 0.5
	compactionBatch    = 512
)

func (e *Engine) maintenanceLoop() {
	defer e.wg.Done()

	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()

	lastCompaction := time.Now()
	lastSweep := time.Now()
	for {
		select {
		case <-e.stop:
			return
		case <-ticker.C:
		}

		if time.Since(lastSweep) >= idleSweepInterval {
			lastSweep = time.Now()
			e.DropIdleIndexes(IndexIdleAfter)
		}

		if e.checkpointDue() {
			if err := e.checkpoint(false); err != nil {
				engineLog.Error("checkpoint failed", "err", err)
			}
		}

		if time.Since(lastCompaction) >= compactionInterval {
			lastCompaction = time.Now()
			if n, err := e.Compact(1); err != nil {
				engineLog.Error("compaction failed", "err", err)
			} else if n > 0 {
				if err := e.checkpoint(false); err != nil {
					engineLog.Error("checkpoint failed", "err", err)
				}
			}
		}
	}
}

func (e *Engine) LogSinceCheckpoint() int64 {
	pos := e.j.Position()
	e.ckptMu.Lock()
	ck := e.checkpointPos
	e.ckptMu.Unlock()

	if ck.Segment == 0 {
		ck = journal.Position{Segment: 1, Offset: 16}
	}

	if pos.Segment == ck.Segment {
		return int64(pos.Offset) - int64(ck.Offset)
	}

	size := e.opts.SegmentSize
	if size <= 0 {
		size = journal.DefaultSegmentSize
	}

	return int64(pos.Segment-ck.Segment-1)*size + (size - int64(ck.Offset)) + int64(pos.Offset)
}

func (e *Engine) checkpointDue() bool {
	since := e.LogSinceCheckpoint()
	if since <= 0 {
		return false
	}

	if e.opts.MaxLogBytes > 0 && since >= e.opts.MaxLogBytes {
		return true
	}

	e.ckptMu.Lock()
	at := e.checkpointAt
	e.ckptMu.Unlock()

	return e.opts.CheckpointEvery > 0 && time.Since(at) >= e.opts.CheckpointEvery
}

func (e *Engine) Compact(maxSegments int) (int, error) {
	livePtr := e.segmentLive.Load()
	if livePtr == nil {
		return 0, nil
	}
	live := *livePtr

	segments, err := e.j.Segments()
	if err != nil {
		return 0, err
	}

	e.ckptMu.Lock()
	ckpt := e.checkpointPos
	e.ckptMu.Unlock()

	done := 0
	for _, seg := range segments {
		if done >= maxSegments {
			break
		}

		if seg.ID >= ckpt.Segment || seg.Size <= 0 {
			continue
		}

		ratio := float64(live[seg.ID]) / float64(seg.Size)
		if live[seg.ID] == 0 || ratio >= compactionRatio {
			continue
		}

		start := time.Now()
		moved, err := e.compactSegment(seg.ID)
		if err != nil {
			return done, err
		}

		done++
		e.stats.compactedSegments.Add(1)
		e.stats.compactedBytes.Add(seg.Size)
		engineLog.Debug("log segment compacted", "segment", seg.ID, "moved", moved, "bytes", seg.Size, "duration_ms", log.Since(start))
	}

	return done, nil
}

func (e *Engine) compactSegment(id uint32) (int, error) {
	moved := 0
	err := e.j.ScanSegment(id, func(recs []journal.Record) error {
		docs := map[string][]journal.Record{}
		kvs := []journal.Record{}
		for _, rec := range recs {
			switch rec.Kind {
			case journal.KindDocPut, journal.KindDocMove:
				docs[rec.Entity] = append(docs[rec.Entity], rec)
			case journal.KindKVPut, journal.KindKVMove:
				kvs = append(kvs, rec)
			}
		}

		for name, list := range docs {
			n, err := e.moveDocs(name, list)
			if err != nil {
				return err
			}
			moved += n
		}

		for _, rec := range kvs {
			ok, err := e.moveKV(rec)
			if err != nil {
				return err
			}
			if ok {
				moved++
			}
		}

		return nil
	})

	return moved, err
}

func (e *Engine) moveDocs(name string, recs []journal.Record) (int, error) {
	ent := e.entity(name, false)
	if ent == nil {
		return 0, nil
	}

	moved := 0
	for start := 0; start < len(recs); start += compactionBatch {
		end := min(start+compactionBatch, len(recs))

		ent.apply.Lock()
		r := ent.root.Load()
		batch := []journal.Record{}
		for _, rec := range recs[start:end] {
			s, ok := r.ids.Get(rec.Key)
			if !ok || s.lsn != rec.LSN || rec.Gen != r.gen {
				continue
			}

			batch = append(batch, journal.Record{Kind: journal.KindDocMove, Entity: name, Key: rec.Key, Gen: r.gen, Value: append([]byte(nil), rec.Value...)})
		}

		if len(batch) == 0 {
			ent.apply.Unlock()
			continue
		}

		if _, err := e.j.Append(batch); err != nil {
			ent.apply.Unlock()
			return moved, err
		}

		d := e.newDraft(ent)
		for i := range batch {
			d.put(&batch[i], nil, false)
		}
		d.publish()
		ent.apply.Unlock()

		moved += len(batch)
	}

	return moved, nil
}

func (e *Engine) moveKV(rec journal.Record) (bool, error) {
	sh := e.kv.shard(rec.Key)
	sh.mu.Lock()
	defer sh.mu.Unlock()

	entry, ok := sh.m[rec.Key]
	if !ok || entry.lsn != rec.LSN {
		return false, nil
	}

	recs := []journal.Record{{Kind: journal.KindKVMove, Key: rec.Key, Value: entry.value, ExpiresAt: entry.expiresAt}}
	if _, err := e.j.Append(recs); err != nil {
		return false, err
	}

	moved := *entry
	moved.lsn = recs[0].LSN
	moved.loc = recs[0].Loc
	sh.m[rec.Key] = &moved

	return true, nil
}

type EngineStats struct {
	Checkpoints         int64
	LastCheckpointMs    float64
	LogSinceCheckpoint  int64
	CompactedSegments   int64
	CompactedBytes      int64
	Conflicts           int64
	ValueCacheBytes     int64
	ValueCacheHits      int64
	ValueCacheMisses    int64
	Segments            int
	LogBytes            int64
	LiveBytes           int64
	PendingBytes        int
	Inline              bool
	Documents           int64
	ResidentStructBytes int64
}

func (e *Engine) Stats() EngineStats {
	s := EngineStats{
		Checkpoints:        e.stats.checkpoints.Load(),
		LastCheckpointMs:   float64(e.stats.checkpointNanos.Load()) / 1e6,
		LogSinceCheckpoint: e.LogSinceCheckpoint(),
		CompactedSegments:  e.stats.compactedSegments.Load(),
		CompactedBytes:     e.stats.compactedBytes.Load(),
		Conflicts:          e.stats.conflicts.Load(),
		ValueCacheHits:     e.cache.hits.Load(),
		ValueCacheMisses:   e.cache.misses.Load(),
		PendingBytes:       e.j.PendingBytes(),
		Inline:             e.inline,
	}

	if !e.inline {
		s.ValueCacheBytes = e.cache.size()
	}

	if segs, err := e.j.Segments(); err == nil {
		s.Segments = len(segs)
		for _, seg := range segs {
			s.LogBytes += seg.Size
		}
	}

	if live := e.segmentLive.Load(); live != nil {
		for _, b := range *live {
			s.LiveBytes += b
		}
	}

	e.catalogMu.RLock()
	for _, ent := range e.entities {
		r := ent.root.Load()
		n := int64(r.slots.Len())
		s.Documents += n
		s.ResidentStructBytes += n * 160
		for _, idx := range r.indexes {
			s.ResidentStructBytes += int64(idx.tree.Len()) * 48
		}
	}
	e.catalogMu.RUnlock()

	return s
}
