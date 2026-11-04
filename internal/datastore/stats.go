package datastore

import "time"

type EngineStats struct {
	Flushes           int64
	LastFlushMs       float64
	UnflushedLogBytes int64
	MemTableBytes     int64
	WALBytes          int64
	LiveBytes         int64
	DiskBytes         int64
	BlockCacheBytes   int64
	BlockCacheLimit   int64
	BlockCacheHits    int64
	BlockCacheMisses  int64
	Conflicts         int64
	Documents         int64
	Compactions       int64
	CompactionDebt    int64
	CompactingBytes   int64
	L0Files           int64
	ReadAmp           int
	WriteStalls       int64
	StallMs           float64
	OpenSnapshots     int
	BuiltDocuments    int64
	BackgroundErrors  int64
}

func (e *Engine) Stats() EngineStats {
	s := EngineStats{
		Flushes:          e.stats.flushes.Load(),
		LastFlushMs:      float64(e.stats.flushNanos.Load()) / 1e6,
		Conflicts:        e.stats.conflicts.Load(),
		WriteStalls:      e.stats.writeStalls.Load(),
		StallMs:          float64(e.stats.stallNanos.Load()) / 1e6,
		BuiltDocuments:   e.stats.builtDocs.Load(),
		BackgroundErrors: e.stats.backgroundErrs.Load(),
		BlockCacheLimit:  e.opts.CacheBytes,
	}

	if e.stats.stalled.Load() {
		s.StallMs += float64(time.Now().UnixNano()-e.stats.stallStart.Load()) / 1e6
	}

	for _, ent := range e.allEntities() {
		if rec := ent.rec.Load(); rec != nil {
			s.Documents += rec.count
		}
	}

	if !e.enter() {
		return s
	}
	m := e.db.Metrics()
	e.leave()

	s.UnflushedLogBytes = int64(m.WAL.Size)
	s.MemTableBytes = int64(m.MemTable.Size)
	s.WALBytes = int64(m.WAL.PhysicalSize)
	s.LiveBytes = int64(m.Table.Local.LiveSize)
	s.DiskBytes = int64(m.DiskSpaceUsage())
	s.BlockCacheBytes = m.BlockCache.Size
	s.BlockCacheHits = m.BlockCache.Hits
	s.BlockCacheMisses = m.BlockCache.Misses
	s.Compactions = m.Compact.Count
	s.CompactionDebt = int64(m.Compact.EstimatedDebt)
	s.CompactingBytes = m.Compact.InProgressBytes
	s.L0Files = m.Levels[0].TablesCount
	s.ReadAmp = m.ReadAmp()
	s.OpenSnapshots = m.Snapshots.Count

	return s
}

func (e *Engine) LogSinceCheckpoint() int64 {
	if !e.enter() {
		return 0
	}
	defer e.leave()

	return int64(e.db.Metrics().WAL.Size)
}
