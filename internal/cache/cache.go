package cache

import (
	"crypto/sha256"
	"sort"
	"strconv"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/log"
)

type cacheItem struct {
	v   []byte
	exp int64
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
}

var CacheStore *cacheStore

func InitCache(ttl time.Duration) {
	CacheStore = &cacheStore{
		entities: make(map[string]*cacheEntity),
		ttl:      ttl,
	}

	log.Info("API Cache initialized with TTL of ", ttl.String())
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
		return nil
	}
	e.mu.RLock()
	it, ok := e.data[key]
	e.mu.RUnlock()
	if !ok {
		return nil
	}
	return it.v
}

func (s *cacheStore) Set(entity string, hash []byte, value []byte) {
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
	exp := time.Now().Add(s.ttl).UnixNano()
	e.mu.Lock()
	e.data[key] = cacheItem{v: value, exp: exp}
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
	if !ok {
		return nil
	}
	return it.v
}

func (s *cacheStore) SetById(entity, id string, value []byte) {
	s.mu.Lock()
	e, exists := s.entities[entity]
	if !exists {
		e = &cacheEntity{data: make(map[[32]byte]cacheItem), ids: make(map[string]cacheItem)}
		s.entities[entity] = e
	}
	s.mu.Unlock()
	exp := time.Now().Add(s.ttl).UnixNano()
	e.mu.Lock()
	e.ids[id] = cacheItem{v: value, exp: exp}
	e.mu.Unlock()
}

func (s *cacheStore) Purge(entity string) {
	s.mu.Lock()
	delete(s.entities, entity)
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
			}
		}
		for id, it := range e.ids {
			if now > it.exp {
				delete(e.ids, id)
			}
		}
		e.mu.Unlock()
	}
}
