package datastore

import "sync"

const changeLogSize = 1 << 16

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

	n := c.next
	if c.full {
		n = len(c.buf)
	}

	seen := map[uint32]bool{}
	var out []uint32
	for i := 0; i < n; i++ {
		ch := c.buf[i]
		if ch.lsn > from && ch.lsn <= upTo && !seen[ch.seq] {
			seen[ch.seq] = true
			out = append(out, ch.seq)
		}
	}

	return out, true
}

func (s *Snapshot) ChangedSince(lsn uint64) ([]uint32, bool) {
	if s.ent == nil {
		return nil, false
	}

	return s.ent.changes.since(lsn, s.r.version)
}
