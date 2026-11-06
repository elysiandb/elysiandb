package datastore

import (
	"hash/fnv"
	"sort"
	"sync"
	"sync/atomic"
)

const (
	changeLogSize   = 1 << 16
	stampSlots      = 1 << 14
	maxWatchedField = 16
)

type change struct {
	lsn uint64
	seq uint32
}

type changeLog struct {
	mu      sync.Mutex
	buf     []change
	next    int
	full    bool
	covered uint64
}

func (c *changeLog) add(since, lsn uint64, seq uint32) {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.buf == nil {
		c.buf = make([]change, changeLogSize)
		c.covered = since
	}

	if c.full {
		c.covered = max(c.covered, c.buf[c.next].lsn)
	}

	c.buf[c.next] = change{lsn: lsn, seq: seq}
	c.next++
	if c.next == len(c.buf) {
		c.next = 0
		c.full = true
	}
}

func (c *changeLog) reset() {
	c.mu.Lock()
	c.buf, c.next, c.full, c.covered = nil, 0, false, 0
	c.mu.Unlock()
}

func (c *changeLog) since(from, upTo uint64) ([]uint32, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.buf == nil || from < c.covered {
		return nil, false
	}

	n, oldest := c.next, 0
	if c.full {
		n, oldest = len(c.buf), c.next
	}

	at := func(i int) change { return c.buf[(oldest+i)%len(c.buf)] }
	start := sort.Search(n, func(i int) bool { return at(i).lsn > from })

	var out []uint32
	var seen map[uint32]bool
	for i := start; i < n; i++ {
		ch := at(i)
		if ch.lsn > upTo {
			break
		}

		if n-start > 1 {
			if seen == nil {
				seen = make(map[uint32]bool, min(n-start, 256))
			}
			if seen[ch.seq] {
				continue
			}
			seen[ch.seq] = true
		}

		out = append(out, ch.seq)
	}

	return out, true
}

func (s *Snapshot) ChangedSince(lsn uint64) ([]uint32, bool) {
	if s.ent == nil {
		return nil, false
	}

	return s.ent.changes.since(lsn, s.r.version)
}

type fieldStamps struct {
	floor atomic.Uint64
	slots [stampSlots]atomic.Uint64
}

type pendingStamp struct {
	table *fieldStamps
	slot  uint32
}

func stampSlot(key []byte) uint32 {
	h := fnv.New32a()
	_, _ = h.Write(key)

	return h.Sum32() % stampSlots
}

func (e *Engine) WatchField(name, field string) {
	ent := e.entity(name, false)
	if ent == nil {
		return
	}

	ent.apply.Lock()
	defer ent.apply.Unlock()

	current := ent.watched.Load()
	if current != nil {
		if _, ok := (*current)[field]; ok || len(*current) >= maxWatchedField {
			return
		}
	}

	next := map[string]*fieldStamps{}
	if current != nil {
		for k, v := range *current {
			next[k] = v
		}
	}

	st := &fieldStamps{}
	st.floor.Store(ent.root.Load().version)
	next[field] = st
	ent.watched.Store(&next)
}

func (s *Snapshot) FieldVersion(field string, terms [][]byte) (uint64, bool) {
	if s.ent == nil {
		return 0, false
	}

	watched := s.ent.watched.Load()
	if watched == nil {
		return 0, false
	}

	st, ok := (*watched)[field]
	if !ok {
		return 0, false
	}

	version := st.floor.Load()
	for _, term := range terms {
		version = max(version, st.slots[stampSlot(term)].Load())
	}

	return version, true
}

func (d *draft) stampChange(oldDoc, newDoc map[string]any, oldKnown bool) {
	watched := d.ent.watched.Load()
	if watched == nil {
		return
	}

	for field, st := range *watched {
		if !oldKnown {
			d.floors = append(d.floors, st)
		}

		for _, key := range FilterKeys(oldDoc, field) {
			d.stamps = append(d.stamps, pendingStamp{table: st, slot: stampSlot(key)})
		}

		for _, key := range FilterKeys(newDoc, field) {
			d.stamps = append(d.stamps, pendingStamp{table: st, slot: stampSlot(key)})
		}
	}
}

func (d *draft) applyStamps() {
	lsn := d.r.version
	for _, s := range d.stamps {
		s.table.slots[s.slot].Store(lsn)
	}

	for _, st := range d.floors {
		st.floor.Store(lsn)
	}
}
