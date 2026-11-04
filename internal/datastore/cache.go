package datastore

import (
	"sync"
	"sync/atomic"
)

type valueCache struct {
	shards []*cacheShard
	hits   atomic.Int64
	misses atomic.Int64
}

type cacheShard struct {
	mu      sync.Mutex
	index   map[uint64]int
	entries []cacheEntry
	free    []int
	hand    int
	bytes   int64
	limit   int64
}

type cacheEntry struct {
	lsn  uint64
	data []byte
	ref  bool
	used bool
}

func newValueCache(limit int64, shards int) *valueCache {
	if shards < 1 {
		shards = 1
	}

	c := &valueCache{shards: make([]*cacheShard, shards)}
	for i := range c.shards {
		c.shards[i] = &cacheShard{index: map[uint64]int{}, limit: max(limit/int64(shards), 1)}
	}

	return c
}

func (c *valueCache) shard(lsn uint64) *cacheShard {
	return c.shards[(lsn*0x9E3779B97F4A7C15>>32)%uint64(len(c.shards))]
}

func (c *valueCache) get(lsn uint64) ([]byte, bool) {
	s := c.shard(lsn)
	s.mu.Lock()
	i, ok := s.index[lsn]
	var data []byte
	if ok {
		s.entries[i].ref = true
		data = s.entries[i].data
	}
	s.mu.Unlock()

	if ok {
		c.hits.Add(1)
	} else {
		c.misses.Add(1)
	}

	return data, ok
}

func (c *valueCache) put(lsn uint64, data []byte) {
	s := c.shard(lsn)
	s.mu.Lock()
	defer s.mu.Unlock()

	if _, ok := s.index[lsn]; ok {
		return
	}

	size := int64(len(data))
	if size > s.limit {
		return
	}

	for s.bytes+size > s.limit && len(s.index) > 0 {
		s.evictOne()
	}

	var i int
	if n := len(s.free); n > 0 {
		i = s.free[n-1]
		s.free = s.free[:n-1]
	} else {
		s.entries = append(s.entries, cacheEntry{})
		i = len(s.entries) - 1
	}

	s.entries[i] = cacheEntry{lsn: lsn, data: data, ref: false, used: true}
	s.index[lsn] = i
	s.bytes += size
}

func (s *cacheShard) evictOne() {
	for {
		if s.hand >= len(s.entries) {
			s.hand = 0
		}

		e := &s.entries[s.hand]
		if !e.used {
			s.hand++
			continue
		}

		if e.ref {
			e.ref = false
			s.hand++
			continue
		}

		delete(s.index, e.lsn)
		s.bytes -= int64(len(e.data))
		*e = cacheEntry{}
		s.free = append(s.free, s.hand)
		s.hand++

		return
	}
}

func (c *valueCache) drop(lsn uint64) int64 {
	s := c.shard(lsn)
	s.mu.Lock()
	defer s.mu.Unlock()

	i, ok := s.index[lsn]
	if !ok {
		return 0
	}

	size := int64(len(s.entries[i].data))
	s.bytes -= size
	s.entries[i] = cacheEntry{}
	s.free = append(s.free, i)
	delete(s.index, lsn)

	return size
}

func (c *valueCache) size() int64 {
	total := int64(0)
	for _, s := range c.shards {
		s.mu.Lock()
		total += s.bytes
		s.mu.Unlock()
	}

	return total
}
