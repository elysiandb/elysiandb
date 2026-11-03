package datastore

import (
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	xxhash "github.com/cespare/xxhash/v2"
	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/ptree"
)

type kvEntry struct {
	value     []byte
	expiresAt int64
	lsn       uint64
	loc       journal.Location
}

type kvShard struct {
	mu sync.RWMutex
	m  map[string]*kvEntry
}

type kvStore struct {
	shards  []*kvShard
	mask    uint64
	keysMu  sync.Mutex
	keys    atomic.Pointer[ptree.Tree[string]]
	count   atomic.Int64
	withTTL atomic.Int64
	wheelMu sync.Mutex
	wheel   map[int64]map[string]struct{}
}

func newKVStore(shards int) *kvStore {
	n := 1
	for n < shards {
		n <<= 1
	}

	s := &kvStore{shards: make([]*kvShard, n), mask: uint64(n - 1), wheel: map[int64]map[string]struct{}{}}
	for i := range s.shards {
		s.shards[i] = &kvShard{m: map[string]*kvEntry{}}
	}
	s.keys.Store(&ptree.Tree[string]{})

	return s
}

func (s *kvStore) shard(key string) *kvShard {
	return s.shards[xxhash.Sum64String(key)&s.mask]
}

func (s *kvStore) lockAll() {
	for _, sh := range s.shards {
		sh.mu.Lock()
	}
}

func (s *kvStore) unlockAll() {
	for i := len(s.shards) - 1; i >= 0; i-- {
		s.shards[i].mu.Unlock()
	}
}

func lowerKey(key string) string {
	return strings.ToLower(key) + "\x00" + key
}

func (s *kvStore) indexKey(key string, add bool) {
	s.keysMu.Lock()
	t := *s.keys.Load()
	if add {
		t, _ = t.Put(lowerKey(key), key)
	} else {
		t, _ = t.Delete(lowerKey(key))
	}
	s.keys.Store(&t)
	s.keysMu.Unlock()
}

func (s *kvStore) schedule(key string, expiresAt int64) {
	if expiresAt <= 0 {
		return
	}

	s.wheelMu.Lock()
	b := s.wheel[expiresAt]
	if b == nil {
		b = map[string]struct{}{}
		s.wheel[expiresAt] = b
	}
	b[key] = struct{}{}
	s.wheelMu.Unlock()
}

func (s *kvStore) setLocked(sh *kvShard, key string, entry *kvEntry) {
	old, existed := sh.m[key]
	sh.m[key] = entry

	if !existed {
		s.count.Add(1)
		s.indexKey(key, true)
	}

	hadTTL := existed && old.expiresAt > 0
	switch {
	case entry.expiresAt > 0 && !hadTTL:
		s.withTTL.Add(1)
	case entry.expiresAt <= 0 && hadTTL:
		s.withTTL.Add(-1)
	}

	s.schedule(key, entry.expiresAt)
}

func (s *kvStore) deleteLocked(sh *kvShard, key string) bool {
	old, existed := sh.m[key]
	if !existed {
		return false
	}

	delete(sh.m, key)
	s.count.Add(-1)
	if old.expiresAt > 0 {
		s.withTTL.Add(-1)
	}
	s.indexKey(key, false)

	return true
}

func (s *kvStore) applyPut(rec *journal.Record, replay bool) {
	sh := s.shard(rec.Key)
	sh.mu.Lock()
	defer sh.mu.Unlock()

	old, exists := sh.m[rec.Key]
	if replay && exists && old.lsn >= rec.LSN {
		return
	}

	if rec.Kind == journal.KindKVMove {
		if !exists {
			return
		}

		moved := *old
		moved.lsn = rec.LSN
		moved.loc = rec.Loc
		sh.m[rec.Key] = &moved

		return
	}

	if replay && rec.ExpiresAt > 0 && rec.ExpiresAt <= time.Now().Unix() {
		s.deleteLocked(sh, rec.Key)
		return
	}

	s.setLocked(sh, rec.Key, &kvEntry{value: append([]byte(nil), rec.Value...), expiresAt: rec.ExpiresAt, lsn: rec.LSN, loc: rec.Loc})
}

func (s *kvStore) applyDelete(key string) {
	sh := s.shard(key)
	sh.mu.Lock()
	s.deleteLocked(sh, key)
	sh.mu.Unlock()
}

func (s *kvStore) clearLocked(lsn uint64) {
	for _, sh := range s.shards {
		sh.m = map[string]*kvEntry{}
	}

	s.count.Store(0)
	s.withTTL.Store(0)
	s.keysMu.Lock()
	s.keys.Store(&ptree.Tree[string]{})
	s.keysMu.Unlock()
	s.wheelMu.Lock()
	s.wheel = map[int64]map[string]struct{}{}
	s.wheelMu.Unlock()
}

func (e *Engine) KVGet(key string) ([]byte, int64, bool) {
	sh := e.kv.shard(key)
	sh.mu.RLock()
	entry, ok := sh.m[key]
	sh.mu.RUnlock()
	if !ok {
		return nil, 0, false
	}

	return entry.value, entry.expiresAt, true
}

func (e *Engine) KVExpiration(key string) (int64, bool) {
	_, exp, ok := e.KVGet(key)
	if !ok || exp <= 0 {
		return 0, false
	}

	return exp, true
}

func (e *Engine) KVPut(key string, value []byte, expiresAt int64) error {
	if e.closed.Load() {
		return ErrClosed
	}

	sh := e.kv.shard(key)
	sh.mu.Lock()
	recs := []journal.Record{{Kind: journal.KindKVPut, Key: key, Value: value, ExpiresAt: expiresAt}}
	ticket, err := e.j.Append(recs)
	if err != nil {
		sh.mu.Unlock()
		return err
	}

	e.kv.setLocked(sh, key, &kvEntry{value: append([]byte(nil), value...), expiresAt: expiresAt, lsn: recs[0].LSN, loc: recs[0].Loc})
	sh.mu.Unlock()

	if err := ticket.Wait(); err != nil {
		engineLog.Error("log write failed, the change stays in memory and will be retried", "err", err)
	}

	return nil
}

func (e *Engine) KVDelete(key string) bool {
	return e.kvDelete(key, journal.KindKVDelete, 0)
}

func (e *Engine) kvDelete(key string, kind journal.Kind, onlyIfExpiresAt int64) bool {
	if e.closed.Load() {
		return false
	}

	sh := e.kv.shard(key)
	sh.mu.Lock()
	entry, ok := sh.m[key]
	if !ok || (onlyIfExpiresAt != 0 && entry.expiresAt != onlyIfExpiresAt) {
		sh.mu.Unlock()
		return false
	}

	ticket, err := e.j.Append([]journal.Record{{Kind: kind, Key: key}})
	if err != nil {
		sh.mu.Unlock()
		return false
	}

	e.kv.deleteLocked(sh, key)
	sh.mu.Unlock()

	if kind == journal.KindKVDelete {
		if err := ticket.Wait(); err != nil {
			engineLog.Error("log write failed, the change stays in memory and will be retried", "err", err)
		}
	}

	return true
}

func (e *Engine) KVReset() error {
	if e.closed.Load() {
		return ErrClosed
	}

	e.kv.lockAll()
	ticket, err := e.j.Append([]journal.Record{{Kind: journal.KindKVReset}})
	if err != nil {
		e.kv.unlockAll()
		return err
	}

	e.kv.clearLocked(0)
	e.kv.unlockAll()

	return ticket.Wait()
}

func (e *Engine) KVCount() (int64, int64) {
	return e.kv.count.Load(), e.kv.withTTL.Load()
}

func (e *Engine) KVKeys() []string {
	t := e.kv.keys.Load()
	keys := make([]string, 0, t.Len())
	t.AscendAt(0, func(_ string, key string) bool {
		keys = append(keys, key)
		return true
	})
	sort.Strings(keys)

	return keys
}

func literalPrefix(pattern string) string {
	if i := strings.IndexAny(pattern, "*\\"); i >= 0 {
		return pattern[:i]
	}

	return pattern
}

func (e *Engine) KVMatch(pattern string, limit int, fn func(key string) bool) {
	t := e.kv.keys.Load()
	lower := strings.ToLower(literalPrefix(pattern))
	n := 0
	t.AscendFrom(lower, func(k string, key string) bool {
		if !strings.HasPrefix(k, lower) {
			return false
		}

		if !docpath.MatchGlob(pattern, key) {
			return true
		}

		n++

		return fn(key) && (limit <= 0 || n < limit)
	})
}

func (e *Engine) expiryLoop() {
	defer e.wg.Done()

	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-e.stop:
			return
		case <-ticker.C:
			e.ExpireDue(time.Now().Unix())
		}
	}
}

func (e *Engine) ExpireDue(now int64) int {
	e.kv.wheelMu.Lock()
	due := []int64{}
	for ts := range e.kv.wheel {
		if ts <= now {
			due = append(due, ts)
		}
	}
	sort.Slice(due, func(i, j int) bool { return due[i] < due[j] })

	buckets := make([]map[string]struct{}, 0, len(due))
	for _, ts := range due {
		buckets = append(buckets, e.kv.wheel[ts])
		delete(e.kv.wheel, ts)
	}
	e.kv.wheelMu.Unlock()

	expired := 0
	for i, b := range buckets {
		for key := range b {
			if e.kvDelete(key, journal.KindKVExpire, due[i]) {
				expired++
			}
		}
	}

	if expired > 0 && e.opts.OnKVExpired != nil {
		e.opts.OnKVExpired(expired)
	}

	return expired
}
