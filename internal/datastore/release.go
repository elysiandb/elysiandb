package datastore

import (
	"runtime/debug"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/ptree"
)

const (
	returnMemoryAbove = 64 << 20
	returnMemoryEvery = time.Minute
)

var lastMemoryReturn atomic.Int64

func (e *Engine) releaseValues(trees []ptree.Tree[*slot]) {
	run := func() {
		var freed int64
		for _, t := range trees {
			t.AscendAt(0, func(_ string, s *slot) bool {
				if e.inline {
					freed += int64(len(s.data))
				} else {
					freed += e.cache.drop(s.lsn)
				}
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
