package cache

import (
	"crypto/sha256"
	"sort"
	"strconv"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
)

type cacheItem struct {
	v    []byte
	exp  int64
	deps []dependency
}

type dependency struct {
	entity  string
	version string
}

var VersionOf func(entity string) string

func captureDeps(entities []string) []dependency {
	if VersionOf == nil || len(entities) == 0 {
		return nil
	}

	deps := make([]dependency, 0, len(entities))
	for _, entity := range entities {
		deps = append(deps, dependency{entity: entity, version: VersionOf(entity)})
	}

	return deps
}

func (it cacheItem) fresh(now int64) bool {
	if now > it.exp {
		return false
	}

	for _, d := range it.deps {
		if VersionOf(d.entity) != d.version {
			return false
		}
	}

	return true
}

type cacheEntity struct {
	mu   sync.RWMutex
	data map[[32]byte]cacheItem
	ids  map[string]cacheItem
}

type cacheStore struct {
	mu       sync.RWMutex
	entities map[string]*cacheEntity
	ttl      time.Duration
	maxBytes int64
	maxEntry int64
	bytes    atomic.Int64
}

const (
	DefaultMaxMB      = 128
	DefaultMaxEntryKB = 1024
)

var Limits = func() (int64, int64) {
	maxMB, maxEntryKB := int64(DefaultMaxMB), int64(DefaultMaxEntryKB)
	if cfg := globals.GetConfig(); cfg != nil {
		if cfg.Api.Cache.MaxMB > 0 {
			maxMB = int64(cfg.Api.Cache.MaxMB)
		}

		if cfg.Api.Cache.MaxEntryKB > 0 {
			maxEntryKB = int64(cfg.Api.Cache.MaxEntryKB)
		}
	}

	return maxMB << 20, maxEntryKB << 10
}

var (
	CacheStore *cacheStore
	currentMu  sync.RWMutex
)

func InitCache(ttl time.Duration) {
	if ttl <= 0 {
		ttl = time.Second
	}

	maxBytes, maxEntry := Limits()
	store := &cacheStore{
		entities: make(map[string]*cacheEntity),
		ttl:      ttl,
		maxBytes: maxBytes,
		maxEntry: maxEntry,
	}

	currentMu.Lock()
	CacheStore = store
	currentMu.Unlock()

	log.Debug("api cache initialized", "component", "cache", "ttl", ttl)
}

func (s *cacheStore) Entries() int {
	s.mu.RLock()
	entities := make([]*cacheEntity, 0, len(s.entities))
	for _, e := range s.entities {
		entities = append(entities, e)
	}
	s.mu.RUnlock()

	total := 0
	for _, e := range entities {
		e.mu.RLock()
		total += len(e.data) + len(e.ids)
		e.mu.RUnlock()
	}

	return total
}

func (s *cacheStore) Get(entity string, hash []byte) []byte {
	if len(hash) != 32 {
		return nil
	}

	var key [32]byte
	copy(key[:], hash)
	s.mu.RLock()
	e, exists := s.entities[entity]
	s.mu.RUnlock()

	if !exists {
		metrics.CacheMiss()
		return nil
	}

	e.mu.RLock()
	it, ok := e.data[key]
	e.mu.RUnlock()

	if !ok || !it.fresh(time.Now().UnixNano()) {
		metrics.CacheMiss()
		return nil
	}

	metrics.CacheHit()

	return it.v
}

func (s *cacheStore) Accepts(size int) bool {
	return s != nil && int64(size) <= s.maxEntry
}

func (s *cacheStore) admit(size int) bool {
	if int64(size) > s.maxEntry {
		return false
	}

	if s.bytes.Add(int64(size)) > s.maxBytes {
		s.evict()
	}

	return true
}

func (s *cacheStore) evict() {
	target := s.maxBytes * 9 / 10

	s.mu.RLock()
	entities := make([]*cacheEntity, 0, len(s.entities))
	for _, e := range s.entities {
		entities = append(entities, e)
	}
	s.mu.RUnlock()

	for _, e := range entities {
		if s.bytes.Load() <= target {
			return
		}

		e.mu.Lock()
		for k, it := range e.data {
			delete(e.data, k)
			s.bytes.Add(-int64(len(it.v)))
			if s.bytes.Load() <= target {
				break
			}
		}

		for id, it := range e.ids {
			if s.bytes.Load() <= target {
				break
			}
			delete(e.ids, id)
			s.bytes.Add(-int64(len(it.v)))
		}
		e.mu.Unlock()
	}
}

func (s *cacheStore) Bytes() int64 {
	if s == nil {
		return 0
	}

	return s.bytes.Load()
}

func (s *cacheStore) Set(entity string, hash, value []byte) {
	s.SetWithDeps(entity, hash, value, nil)
}

func (s *cacheStore) SetWithDeps(entity string, hash, value []byte, entities []string) {
	if len(hash) != 32 {
		return
	}

	var key [32]byte
	copy(key[:], hash)
	s.mu.Lock()
	e, exists := s.entities[entity]

	if !exists {
		e = &cacheEntity{data: make(map[[32]byte]cacheItem), ids: make(map[string]cacheItem)}
		s.entities[entity] = e
	}

	s.mu.Unlock()

	if !s.admit(len(value)) {
		return
	}

	exp := time.Now().Add(s.ttl).UnixNano()
	deps := captureDeps(entities)
	e.mu.Lock()
	if old, ok := e.data[key]; ok {
		s.bytes.Add(-int64(len(old.v)))
	}
	e.data[key] = cacheItem{v: value, exp: exp, deps: deps}
	e.mu.Unlock()
}

func (s *cacheStore) GetById(entity, id string) []byte {
	s.mu.RLock()
	e, exists := s.entities[entity]
	s.mu.RUnlock()

	if !exists {
		return nil
	}

	e.mu.RLock()
	it, ok := e.ids[id]
	e.mu.RUnlock()

	if !ok || !it.fresh(time.Now().UnixNano()) {
		return nil
	}

	return it.v
}

func (s *cacheStore) SetById(entity, id string, value []byte) {
	s.SetByIdWithDeps(entity, id, value, nil)
}

func (s *cacheStore) SetByIdWithDeps(entity, id string, value []byte, entities []string) {
	s.mu.Lock()
	e, exists := s.entities[entity]
	if !exists {
		e = &cacheEntity{data: make(map[[32]byte]cacheItem), ids: make(map[string]cacheItem)}
		s.entities[entity] = e
	}

	s.mu.Unlock()

	if !s.admit(len(value)) {
		return
	}

	exp := time.Now().Add(s.ttl).UnixNano()
	deps := captureDeps(entities)
	e.mu.Lock()
	if old, ok := e.ids[id]; ok {
		s.bytes.Add(-int64(len(old.v)))
	}
	e.ids[id] = cacheItem{v: value, exp: exp, deps: deps}
	e.mu.Unlock()
}

func (s *cacheStore) Purge(entity string) {
	s.mu.Lock()
	e := s.entities[entity]
	delete(s.entities, entity)
	s.mu.Unlock()

	if e != nil {
		s.bytes.Add(-e.size())
	}
}

func (e *cacheEntity) size() int64 {
	e.mu.RLock()
	defer e.mu.RUnlock()

	total := int64(0)
	for _, it := range e.data {
		total += int64(len(it.v))
	}

	for _, it := range e.ids {
		total += int64(len(it.v))
	}

	return total
}

func (s *cacheStore) PurgeAll() {
	s.mu.Lock()
	s.entities = make(map[string]*cacheEntity)
	s.bytes.Store(0)
	s.mu.Unlock()
}

func HashQuery(
	entity string,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	filters map[string]map[string]string,
	fieldsParam string,
	search string,
	includesParam string,
	countOnlyParam bool,
	username string,
) []byte {
	var b []byte
	b = append(b, entity...)
	b = append(b, '|')
	b = append(b, sortField...)
	b = append(b, '|')
	b = append(b, fieldsParam...)
	b = append(b, '|')
	b = append(b, search...)
	b = append(b, '|')
	b = append(b, includesParam...)
	b = append(b, '|')
	b = append(b, username...)
	b = append(b, '|')

	if countOnlyParam {
		b = append(b, '1')
	} else {
		b = append(b, '0')
	}

	b = append(b, '|')

	if sortAscending {
		b = append(b, 'A')
	} else {
		b = append(b, 'D')
	}

	b = append(b, '|')
	b = strconv.AppendInt(b, int64(limit), 10)
	b = append(b, '|')
	b = strconv.AppendInt(b, int64(offset), 10)
	keys := make([]string, 0, len(filters))

	for k := range filters {
		keys = append(keys, k)
	}

	sort.Strings(keys)

	for _, k := range keys {
		ops := filters[k]
		opsKeys := make([]string, 0, len(ops))
		for op := range ops {
			opsKeys = append(opsKeys, op)
		}

		sort.Strings(opsKeys)
		for _, op := range opsKeys {
			b = append(b, '|')
			b = append(b, k...)
			b = append(b, '[')
			b = append(b, op...)
			b = append(b, ']', '=')
			b = append(b, ops[op]...)
		}
	}
	sum := sha256.Sum256(b)

	return sum[:]
}

func CleanExpiredEntries() {
	currentMu.RLock()
	store := CacheStore
	currentMu.RUnlock()

	if store != nil {
		store.CleanExpired()
	}
}

func (s *cacheStore) CleanExpired() {
	s.mu.RLock()
	entities := make(map[string]*cacheEntity, len(s.entities))
	for k, v := range s.entities {
		entities[k] = v
	}

	s.mu.RUnlock()
	now := time.Now().UnixNano()

	for _, e := range entities {
		e.mu.Lock()
		for k, it := range e.data {
			if now > it.exp {
				delete(e.data, k)
				s.bytes.Add(-int64(len(it.v)))
			}
		}

		for id, it := range e.ids {
			if now > it.exp {
				delete(e.ids, id)
				s.bytes.Add(-int64(len(it.v)))
			}
		}

		e.mu.Unlock()
	}
}
