package storage

import (
	"sync"
	"sync/atomic"

	xxhash "github.com/cespare/xxhash/v2"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/recovery"
)

const (
	DataFile           = "elysiandb.json"
	JsonDataFile       = "elysiandbjson.json"
	ExpirationDataFile = "elysiandb.expiration.json"
)

type ExpirationContainer struct {
	Buckets map[int64]*ExpirationBucket
	index   map[string]int64
	mu      sync.RWMutex
	saved   atomic.Bool
}

type ExpirationBucket struct {
	Keys []string
	mu   sync.RWMutex
}

func newExpirationContainer() *ExpirationContainer {
	c := &ExpirationContainer{
		Buckets: make(map[int64]*ExpirationBucket),
		index:   make(map[string]int64),
	}

	c.saved.Store(true)

	return c
}

func (s *ExpirationContainer) CountTotalKeys() uint64 {
	total := uint64(0)
	s.mu.RLock()
	for _, b := range s.Buckets {
		b.mu.RLock()
		total += uint64(len(b.Keys))
		b.mu.RUnlock()
	}

	s.mu.RUnlock()
	return total
}

func (c *ExpirationContainer) put(ts int64, keys []string) {
	for _, k := range keys {
		c.del(k)
	}

	c.mu.Lock()
	b := c.Buckets[ts]
	if b == nil {
		b = &ExpirationBucket{}
		c.Buckets[ts] = b
	}
	c.mu.Unlock()

	b.mu.Lock()
	b.Keys = append(b.Keys, keys...)
	b.mu.Unlock()

	c.mu.Lock()
	if c.index == nil {
		c.index = make(map[string]int64)
	}
	for _, k := range keys {
		c.index[k] = ts
	}
	c.saved.Store(false)
	c.mu.Unlock()
}

func (c *ExpirationContainer) ToMap() map[int64][]string {
	result := make(map[int64][]string)

	c.mu.RLock()
	for k, v := range c.Buckets {
		v.mu.RLock()
		cp := append([]string(nil), v.Keys...)
		v.mu.RUnlock()
		result[k] = cp
	}
	c.mu.RUnlock()

	return result
}

func (c *ExpirationContainer) FromMap(data map[int64][]string) {
	for k, v := range data {
		c.put(k, v)
	}
}

func (c *ExpirationContainer) del(key string) {
	c.mu.RLock()
	ts, ok := c.index[key]
	c.mu.RUnlock()
	if !ok {
		return
	}

	var b *ExpirationBucket
	c.mu.Lock()
	b = c.Buckets[ts]
	delete(c.index, key)
	c.mu.Unlock()

	if b != nil {
		b.mu.Lock()
		for i, k := range b.Keys {
			if k == key {
				b.Keys = append(b.Keys[:i], b.Keys[i+1:]...)
				break
			}
		}
		empty := len(b.Keys) == 0
		b.mu.Unlock()

		if empty {
			c.mu.Lock()
			if c.Buckets[ts] == b {
				delete(c.Buckets, ts)
			}
			c.mu.Unlock()
		}
	}
	c.saved.Store(false)
}

func (c *ExpirationContainer) reset() {
	c.mu.Lock()
	c.Buckets = make(map[int64]*ExpirationBucket)
	c.index = make(map[string]int64)
	c.mu.Unlock()
}

type shard struct {
	mu sync.RWMutex
	m  map[string][]byte
}

type Store struct {
	shards     []*shard
	saved      atomic.Bool
	shardMask  uint64
	shardCount int
}

func NewStore() *Store {
	n := globals.GetConfig().Store.Shards

	s := &Store{
		shards:     make([]*shard, n),
		shardMask:  uint64(n - 1),
		shardCount: n,
	}

	for i := 0; i < n; i++ {
		s.shards[i] = &shard{m: make(map[string][]byte)}
	}

	s.saved.Store(true)

	return s
}

func (s *Store) CountTotalKeys() uint64 {
	total := uint64(0)
	for i := 0; i < s.shardCount; i++ {
		sh := s.shards[i]
		sh.mu.RLock()
		total += uint64(len(sh.m))
		sh.mu.RUnlock()
	}

	return total
}

func (s *Store) reset() {
	for i := 0; i < s.shardCount; i++ {
		sh := s.shards[i]
		sh.mu.Lock()
		sh.m = make(map[string][]byte)
		sh.mu.Unlock()
	}
	s.saved.Store(false)

	if globals.GetConfig().Store.CrashRecovery.Enabled {
		recovery.ClearStoreRecoveryLog()
	}
}

func (s *Store) shardIndex(key string) int {
	h := xxhash.Sum64String(key)
	return int(h & s.shardMask)
}

func (s *Store) get(key string) ([]byte, bool) {
	sh := s.shards[s.shardIndex(key)]
	sh.mu.RLock()
	v, ok := sh.m[key]
	sh.mu.RUnlock()
	if !ok {
		return nil, false
	}

	return v, true
}

func (s *Store) put(key string, value []byte) {
	buf := make([]byte, len(value))
	copy(buf, value)

	sh := s.shards[s.shardIndex(key)]
	sh.mu.Lock()
	sh.m[key] = buf
	sh.mu.Unlock()
	s.saved.Store(false)

	if globals.GetConfig().Store.CrashRecovery.Enabled {
		recovery.LogStorePut(key, buf)
	}
}

func (s *Store) del(key string) {
	sh := s.shards[s.shardIndex(key)]
	sh.mu.Lock()
	delete(sh.m, key)
	sh.mu.Unlock()
	s.saved.Store(false)

	if globals.GetConfig().Store.CrashRecovery.Enabled {
		recovery.LogStoreDelete(key)
	}
}

func (s *Store) Iterate(fn func(k string, v []byte)) {
	for i := 0; i < s.shardCount; i++ {
		sh := s.shards[i]
		sh.mu.RLock()
		for k, v := range sh.m {
			c := make([]byte, len(v))
			copy(c, v)
			fn(k, c)
		}

		sh.mu.RUnlock()
	}
}

func (s *Store) FromMap(src map[string][]byte) {
	for k, v := range src {
		sh := s.shards[s.shardIndex(k)]
		sh.mu.Lock()
		buf := make([]byte, len(v))
		copy(buf, v)
		sh.m[k] = buf
		sh.mu.Unlock()
	}
	s.saved.Store(true)
}

func (s *Store) ToMap() map[string][]byte {
	result := make(map[string][]byte)
	s.Iterate(func(k string, v []byte) {
		result[k] = v
	})

	return result
}
