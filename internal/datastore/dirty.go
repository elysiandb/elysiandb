package datastore

import (
	"time"

	"github.com/taymour/elysiandb/internal/memory"
	"github.com/taymour/elysiandb/internal/pagetree"
)

const (
	defaultDirtyFlushBytes = 256 << 20
	dirtyInterval          = 200 * time.Millisecond
	writerWaitLimit        = 30 * time.Second
)

func (e *Engine) dirtyFlushBytes() int64 {
	if e.opts.DirtyFlushBytes > 0 {
		return e.opts.DirtyFlushBytes + min(memory.Elastic()/2, e.opts.DirtyFlushBytes)
	}

	return defaultDirtyFlushBytes
}

func (e *Engine) DirtyBytes() int64 {
	return e.dirty.Load()
}

func (e *Engine) measureDirty() int64 {
	var n int64
	count := func(_ *pagetree.Ref, p *pagetree.Page) error {
		n += int64(p.Bytes())
		return nil
	}

	for _, ent := range e.allEntities() {
		r := ent.root.Load()
		_ = r.slots.ForEachDirty(count)
		_ = r.ids.ForEachDirty(count)
		for _, idx := range r.indexes {
			if idx.ready {
				_ = idx.tree.ForEachDirty(count)
			}
		}
	}

	e.dirty.Store(n)

	return n
}

func (e *Engine) allEntities() []*entity {
	e.catalogMu.RLock()
	defer e.catalogMu.RUnlock()

	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}

	return ents
}

func (e *Engine) dirtyLoop() {
	defer e.wg.Done()

	ticker := time.NewTicker(dirtyInterval)
	defer ticker.Stop()

	for {
		select {
		case <-e.stop:
			return
		case <-ticker.C:
		}

		if e.measureDirty() >= e.dirtyFlushBytes() {
			e.requestFlush()
		}

		e.followPressure()
		e.checkDisk()
	}
}

func (e *Engine) requestFlush() {
	select {
	case e.flushNow <- struct{}{}:
	default:
	}
}

func (e *Engine) signalFlushed() {
	e.flushedMu.Lock()
	close(e.flushed)
	e.flushed = make(chan struct{})
	e.flushedMu.Unlock()
}

func (e *Engine) waitForDirty() {
	limit := 2 * e.dirtyFlushBytes()
	if !e.opts.Background || e.dirty.Load() < limit || e.WriteRefusal() != nil {
		return
	}

	deadline := time.Now().Add(writerWaitLimit)
	for e.dirty.Load() >= limit && !e.closed.Load() && time.Now().Before(deadline) {
		e.requestFlush()

		e.flushedMu.Lock()
		ch := e.flushed
		e.flushedMu.Unlock()

		select {
		case <-ch:
			e.measureDirty()
		case <-e.stop:
			return
		case <-time.After(dirtyInterval):
		}
	}
}

func scaled(base int64, level memory.Level) int64 {
	if base <= 0 {
		return base
	}

	switch level {
	case memory.High:
		return base / 2
	case memory.Critical:
		return base / 4
	}

	return base
}

func (e *Engine) followPressure() {
	level := memory.Current()
	elastic := memory.Elastic()

	if e.opts.CacheBytes > 0 {
		e.cache.setLimit(scaled(e.opts.CacheBytes+elastic/4, level))
	}

	if e.opts.PageCacheBytes > 0 {
		e.pages.SetLimit(scaled(e.opts.PageCacheBytes+elastic*3/4, level))
	}

	if level == memory.Critical {
		releaseUnderCriticalPressure()
	}
}
