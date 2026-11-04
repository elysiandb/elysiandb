package datastore

import (
	"math"
	"runtime/debug"
	"sort"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/pagetree"
)

const (
	returnMemoryAbove = 64 << 20
	returnMemoryEvery = time.Minute
	warmBatch         = 4096
)

var lastMemoryReturn atomic.Int64

func valueCacheLimit(bytes int64) int64 {
	if bytes <= 0 {
		return math.MaxInt64 / 2
	}

	return bytes
}

func (e *Engine) releaseValues(trees []pagetree.Tree) {
	run := func() {
		var freed int64
		for _, t := range trees {
			t.AscendAt(0, func(k string, v []byte) bool {
				s := decodeSlotValue(keySeq(k), v, true)
				freed += e.cache.drop(s.lsn)
				e.live.add(s.loc.Segment, -int64(s.loc.Length))
				return true
			})
		}

		returnMemory(freed)
	}

	if e.opts.Background {
		go run()
		return
	}

	run()
}

func returnMemory(freed int64) {
	if freed < returnMemoryAbove {
		return
	}

	now := time.Now().UnixNano()
	last := lastMemoryReturn.Load()
	if now-last < int64(returnMemoryEvery) || !lastMemoryReturn.CompareAndSwap(last, now) {
		return
	}

	debug.FreeOSMemory()
}

func (e *Engine) warmValues() {
	e.catalogMu.RLock()
	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}
	e.catalogMu.RUnlock()

	for _, ent := range ents {
		var batch []slot
		flush := func() {
			sort.Slice(batch, func(i, j int) bool {
				if batch[i].loc.Segment != batch[j].loc.Segment {
					return batch[i].loc.Segment < batch[j].loc.Segment
				}
				return batch[i].loc.Offset < batch[j].loc.Offset
			})
			for _, s := range batch {
				if e.closed.Load() {
					return
				}
				_, _ = e.slotBytes(s)
			}
			batch = batch[:0]
		}

		ent.root.Load().slots.AscendAt(0, func(k string, v []byte) bool {
			batch = append(batch, decodeSlotValue(keySeq(k), v, false))
			if len(batch) >= warmBatch {
				flush()
			}
			return !e.closed.Load()
		})
		flush()
	}
}
