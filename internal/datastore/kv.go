package datastore

import (
	"encoding/binary"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	xxhash "github.com/cespare/xxhash/v2"
	"github.com/cockroachdb/pebble/v2"
	"github.com/taymour/elysiandb/internal/docpath"
)

const expireBatch = 4096

type kvShard struct {
	mu sync.Mutex
}

type kvState struct {
	shards  []*kvShard
	mask    uint64
	count   atomic.Int64
	withTTL atomic.Int64
}

func newKVState(shards int) *kvState {
	n := 1
	for n < shards {
		n <<= 1
	}

	s := &kvState{shards: make([]*kvShard, n), mask: uint64(n - 1)}
	for i := range s.shards {
		s.shards[i] = &kvShard{}
	}

	return s
}

func (s *kvState) shard(key string) *kvShard {
	return s.shards[xxhash.Sum64String(key)&s.mask]
}

func (s *kvState) lockAll() {
	for _, sh := range s.shards {
		sh.mu.Lock()
	}
}

func (s *kvState) unlockAll() {
	for i := len(s.shards) - 1; i >= 0; i-- {
		s.shards[i].mu.Unlock()
	}
}

func countPrefix(db *pebble.DB, family byte) (int64, error) {
	it, err := db.NewIter(&pebble.IterOptions{LowerBound: []byte{family}, UpperBound: []byte{family + 1}})
	if err != nil {
		return 0, err
	}

	n := int64(0)
	for it.First(); it.Valid(); it.Next() {
		n++
	}

	return n, it.Close()
}

func (s *kvState) load(db *pebble.DB) error {
	keys, err := countPrefix(db, familyKV)
	if err != nil {
		return err
	}

	withTTL, err := countPrefix(db, familyTTL)
	if err != nil {
		return err
	}

	s.count.Store(keys)
	s.withTTL.Store(withTTL)

	return nil
}

func (s *kvState) clear(b *pebble.Batch) error {
	if err := b.DeleteRange([]byte{familyKV}, []byte{familyKV + 1}, nil); err != nil {
		return err
	}

	return b.DeleteRange([]byte{familyTTL}, []byte{familyTTL + 1}, nil)
}

func (s *kvState) resetCounts() {
	s.count.Store(0)
	s.withTTL.Store(0)
}

func (e *Engine) KVGet(key string) ([]byte, int64, bool) {
	if e.closed.Load() {
		return nil, 0, false
	}

	pk := kvKey(key)
	b, ok, err := e.get(pk)
	if err != nil || !ok {
		return nil, 0, false
	}

	_, value, exp, valid := decodeKVValue(pk, b)
	if !valid {
		return nil, 0, false
	}

	return value, exp, true
}

func (e *Engine) KVExpiration(key string) (int64, bool) {
	_, exp, ok := e.KVGet(key)
	if !ok || exp <= 0 {
		return 0, false
	}

	return exp, true
}

func (e *Engine) KVPut(key string, value []byte, expiresAt int64) error {
	if !e.enter() {
		return ErrClosed
	}
	defer e.leave()

	sh := e.kv.shard(key)
	sh.mu.Lock()

	pk := kvKey(key)
	old, existed, err := e.get(pk)
	if err != nil {
		sh.mu.Unlock()
		return err
	}

	oldExp := int64(0)
	if existed {
		if _, _, exp, ok := decodeKVValue(pk, old); ok {
			oldExp = exp
		}
	}

	b := e.db.NewBatch()
	_ = b.Set(pk, encodeKVValue(nil, key, value, expiresAt), nil)
	if oldExp > 0 && oldExp != expiresAt {
		_ = b.Delete(ttlKey(oldExp, key), nil)
	}
	if expiresAt > 0 {
		_ = b.Set(ttlKey(expiresAt, key), nil, nil)
	}

	a, err := e.apply(b, true)
	if err != nil {
		sh.mu.Unlock()
		return err
	}

	if !existed {
		e.kv.count.Add(1)
	}

	switch {
	case expiresAt > 0 && oldExp <= 0:
		e.kv.withTTL.Add(1)
	case expiresAt <= 0 && oldExp > 0:
		e.kv.withTTL.Add(-1)
	}
	sh.mu.Unlock()

	return a.finish()
}

func (e *Engine) KVDelete(key string) bool {
	return e.kvDelete(key, 0, true)
}

func (e *Engine) kvDelete(key string, onlyIfExpiresAt int64, durable bool) bool {
	if !e.enter() {
		return false
	}
	defer e.leave()

	sh := e.kv.shard(key)
	sh.mu.Lock()

	pk := kvKey(key)
	old, existed, err := e.get(pk)
	if err != nil || !existed {
		sh.mu.Unlock()
		return false
	}

	_, _, exp, _ := decodeKVValue(pk, old)
	if onlyIfExpiresAt != 0 && exp != onlyIfExpiresAt {
		sh.mu.Unlock()
		return false
	}

	b := e.db.NewBatch()
	_ = b.Delete(pk, nil)
	if exp > 0 {
		_ = b.Delete(ttlKey(exp, key), nil)
	}

	a, err := e.apply(b, durable)
	if err != nil {
		sh.mu.Unlock()
		return false
	}

	e.kv.count.Add(-1)
	if exp > 0 {
		e.kv.withTTL.Add(-1)
	}
	sh.mu.Unlock()

	_ = a.finish()

	return true
}

func (e *Engine) KVReset() error {
	if !e.enter() {
		return ErrClosed
	}
	defer e.leave()

	e.kv.lockAll()
	b := e.db.NewBatch()
	if err := e.kv.clear(b); err != nil {
		_ = b.Close()
		e.kv.unlockAll()
		return err
	}

	a, err := e.apply(b, true)
	if err != nil {
		e.kv.unlockAll()
		return err
	}

	e.kv.resetCounts()
	e.kv.unlockAll()

	return a.finish()
}

func (e *Engine) KVCount() (int64, int64) {
	return e.kv.count.Load(), e.kv.withTTL.Load()
}

func (e *Engine) scanKV(lower []byte, fn func(key string) bool) {
	if e.closed.Load() {
		return
	}

	it, err := e.iter(lower, []byte{familyKV + 1})
	if err != nil {
		return
	}
	defer it.Close()

	for it.First(); it.Valid(); it.Next() {
		key, _, _, ok := decodeKVValue(it.Key(), it.Value())
		if !ok {
			continue
		}

		if !fn(key) {
			return
		}
	}
}

func (e *Engine) KVKeys() []string {
	keys := []string{}
	e.scanKV([]byte{familyKV}, func(key string) bool {
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
	lower := strings.ToLower(literalPrefix(pattern))
	seek := kvSeekKey(lower)
	n := 0
	var keys []string
	e.scanKV(seek, func(key string) bool {
		if !strings.HasPrefix(strings.ToLower(key), lower) {
			return false
		}

		if !docpath.MatchGlob(pattern, key) {
			return true
		}

		keys = append(keys, key)
		n++

		return limit <= 0 || n < limit
	})

	for _, key := range keys {
		if !fn(key) {
			return
		}
	}
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

type dueKey struct {
	key string
	exp int64
}

func (e *Engine) dueKeys(now int64) []dueKey {
	if e.closed.Load() {
		return nil
	}

	var upper [9]byte
	upper[0] = familyTTL
	binary.BigEndian.PutUint64(upper[1:], uint64(now+1))
	it, err := e.iter([]byte{familyTTL}, upper[:])
	if err != nil {
		return nil
	}
	defer it.Close()

	out := []dueKey{}
	for it.First(); it.Valid() && len(out) < expireBatch; it.Next() {
		k := it.Key()
		if len(k) < 9 {
			continue
		}
		out = append(out, dueKey{key: string(k[9:]), exp: int64(binary.BigEndian.Uint64(k[1:9]))})
	}

	return out
}

func (e *Engine) ExpireDue(now int64) int {
	expired := 0
	for {
		due := e.dueKeys(now)
		removed := 0
		for _, d := range due {
			if e.kvDelete(d.key, d.exp, false) {
				expired++
				removed++
			} else {
				e.dropTTLEntry(d)
			}
		}

		if len(due) < expireBatch || removed == 0 {
			break
		}
	}

	if expired > 0 && e.opts.OnKVExpired != nil {
		e.opts.OnKVExpired(expired)
	}

	return expired
}

func (e *Engine) dropTTLEntry(d dueKey) {
	if !e.enter() {
		return
	}
	defer e.leave()

	sh := e.kv.shard(d.key)
	sh.mu.Lock()
	defer sh.mu.Unlock()

	if _, exp, ok := e.KVGet(d.key); ok && exp == d.exp {
		return
	}

	_ = e.db.Delete(ttlKey(d.exp, d.key), pebble.NoSync)
}
