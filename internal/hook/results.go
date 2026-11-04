package hook

import (
	"container/list"
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/datastore"
	engine "github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/memory"
)

type result struct {
	version  string
	rows     []row
	bytes    int64
	bySeq    bool
	gen      uint64
	lsn      uint64
	frontier *atomic.Int64
	aggOnce  sync.Once
	agg      *aggregates
}

func (r *result) reusable(seq uint32, lsn uint64) (row, bool) {
	if r == nil || !r.bySeq || lsn == 0 {
		return row{}, false
	}

	i := sort.Search(len(r.rows), func(i int) bool { return r.rows[i].seq >= seq })
	if i < len(r.rows) && r.rows[i].seq == seq && r.rows[i].lsn == lsn {
		return r.rows[i], true
	}

	return row{}, false
}

type cacheItem struct {
	key string
	res *result
	el  *list.Element
}

type flight struct {
	done chan struct{}
	res  *result
	err  error
}

type resultCache struct {
	mu      sync.Mutex
	items   map[string]*cacheItem
	lru     *list.List
	flights map[string]*flight
	hits    atomic.Int64
	misses  atomic.Int64
}

var results = newResultCache()

func newResultCache() *resultCache {
	return &resultCache{items: map[string]*cacheItem{}, lru: list.New(), flights: map[string]*flight{}}
}

func (c *resultCache) lookup(key, version string) (*result, *result) {
	c.mu.Lock()
	defer c.mu.Unlock()

	it, ok := c.items[key]
	if !ok {
		return nil, nil
	}

	if it.res.version == version {
		c.lru.MoveToFront(it.el)
		return it.res, nil
	}

	return nil, it.res
}

func (c *resultCache) store(key string, res *result) {
	limit := memory.HookResults.Effective()
	if limit > 0 && res.bytes > limit {
		return
	}

	c.mu.Lock()
	defer c.mu.Unlock()

	if it, ok := c.items[key]; ok {
		memory.HookResults.Release(it.res.bytes)
		it.res = res
		c.lru.MoveToFront(it.el)
	} else {
		it := &cacheItem{key: key, res: res}
		it.el = c.lru.PushFront(it)
		c.items[key] = it
	}
	memory.HookResults.Add(res.bytes)

	c.evictLocked(memory.HookResults.Effective())
}

func (c *resultCache) evictLocked(limit int64) {
	for limit > 0 && memory.HookResults.Used() > limit && c.lru.Len() > 1 {
		if c.lru.Back() == c.lru.Front() {
			return
		}
		el := c.lru.Back()
		it := el.Value.(*cacheItem)
		c.lru.Remove(el)
		delete(c.items, it.key)
		memory.HookResults.Release(it.res.bytes)
	}
}

func (c *resultCache) shrink() {
	c.mu.Lock()
	c.evictLocked(memory.HookResults.Effective())
	c.mu.Unlock()
}

func (c *resultCache) reset() {
	c.mu.Lock()
	defer c.mu.Unlock()

	for _, it := range c.items {
		memory.HookResults.Release(it.res.bytes)
	}
	c.items = map[string]*cacheItem{}
	c.lru.Init()
}

func (c *resultCache) get(key, version string, guard *api_storage.Guard, build func(prev *result) (*result, error)) (*result, bool, error) {
	if version != "" {
		if res, _ := c.lookup(key, version); res != nil {
			c.hits.Add(1)
			return res, true, nil
		}
	}

	c.misses.Add(1)

	if version == "" {
		res, err := build(nil)
		return res, false, err
	}

	fkey := key + "\x00" + version
	c.mu.Lock()
	if f, ok := c.flights[fkey]; ok {
		c.mu.Unlock()
		return waitFlight(f, guard)
	}

	f := &flight{done: make(chan struct{})}
	c.flights[fkey] = f
	c.mu.Unlock()

	_, prev := c.lookup(key, version)
	f.res, f.err = build(prev)
	if f.err == nil {
		f.res.version = version
		c.store(key, f.res)
	}

	c.mu.Lock()
	delete(c.flights, fkey)
	c.mu.Unlock()
	close(f.done)

	return f.res, false, f.err
}

func waitFlight(f *flight, guard *api_storage.Guard) (*result, bool, error) {
	ticker := time.NewTicker(20 * time.Millisecond)
	defer ticker.Stop()

	for {
		select {
		case <-f.done:
			return f.res, true, f.err
		case <-ticker.C:
			if err := guard.Check(); err != nil {
				return nil, false, err
			}
		}
	}
}

func buildResult(targetEntity string, filters map[string]map[string]string, owner string, bypassAcl bool, guard *api_storage.Guard, prev *result) (*result, error) {
	mode := acl.ReadAll
	if !bypassAcl {
		mode = acl.ReadModeFor(owner, targetEntity)
	}

	res := &result{bySeq: true}
	if mode == acl.ReadNone {
		return res, nil
	}

	b := newRowBuilder()
	keep := func(doc map[string]any) bool {
		return mode != acl.ReadOwned || acl.OwnedBy(doc, owner)
	}

	includes := api_storage.MergeIncludes("", api_storage.ExtractAutoIncludes(filters))
	if !engine.IsEngineInternal() || includes != "" {
		docs, err := listForHook(targetEntity, filters, guard)
		if err != nil {
			return nil, err
		}

		res.bySeq = false
		for _, doc := range docs {
			if keep(doc) {
				res.rows = append(res.rows, b.build(doc, 0, 0, 0))
			}
		}
		res.bytes = b.bytes

		return res, nil
	}

	snap := api_storage.Snapshot(targetEntity)
	if next, ok := prev.advance(snap, filters, keep, b); ok {
		return next, nil
	}

	planned := api_storage.Plan(api_storage.ListQuery{Entity: targetEntity, Filters: filters, Guard: guard, Snapshot: snap})
	if planned.Err != nil {
		return nil, planned.Err
	}
	res.gen, res.lsn = snap.Generation(), snap.Version()
	res.frontier = new(atomic.Int64)

	res.rows = make([]row, 0, len(planned.Docs))
	last := uint32(0)
	for i, d := range planned.Docs {
		if i%1024 == 1023 {
			if err := guard.Check(); err != nil {
				return nil, err
			}
		}

		seq, lsn := d.Seq(), d.LSN()
		if len(res.rows) > 0 && seq <= last {
			res.bySeq = false
		}
		last = seq

		if r, ok := prev.reusable(seq, lsn); ok {
			res.rows = append(res.rows, r)
			b.bytes += int64(r.size)
			continue
		}

		raw, err := d.Bytes()
		if err != nil {
			continue
		}

		doc, err := datastore.DecodeMap(raw)
		if err != nil || doc == nil || !keep(doc) {
			continue
		}

		res.rows = append(res.rows, b.build(doc, seq, lsn, len(raw)))
		if len(res.rows)%1024 == 0 {
			if limit := memory.HookResults.Effective(); limit > 0 && b.bytes > limit {
				memory.HookResults.Refuse()
				return nil, api_storage.ErrBusy
			}
		}
	}

	res.bytes = b.bytes + int64(cap(res.rows))*48
	res.frontier.Store(int64(len(res.rows)))

	return res, nil
}

const rowSliceBytes = 72

func (r *result) advance(snap *datastore.Snapshot, filters map[string]map[string]string, keep func(map[string]any) bool, b *rowBuilder) (*result, bool) {
	if r == nil || !r.bySeq || r.frontier == nil || r.gen != snap.Generation() || r.lsn > snap.Version() {
		return nil, false
	}

	seqs, ok := snap.ChangedSince(r.lsn)
	if !ok || len(seqs) > len(r.rows)/4+64 {
		return nil, false
	}
	sort.Slice(seqs, func(i, j int) bool { return seqs[i] < seqs[j] })

	updates := make(map[uint32]*row, len(seqs))
	for _, seq := range seqs {
		updates[seq] = nil
		d, ok := snap.BySeq(seq)
		if !ok {
			continue
		}

		raw, err := d.Bytes()
		if err != nil {
			return nil, false
		}

		doc, err := datastore.DecodeMap(raw)
		if err != nil || doc == nil || !api_storage.FiltersMatchEntity(doc, filters) || !keep(doc) {
			continue
		}

		built := b.build(doc, seq, d.LSN(), len(raw))
		updates[seq] = &built
	}

	next := &result{bySeq: true, gen: snap.Generation(), lsn: snap.Version(), bytes: r.bytes + b.bytes}
	last := uint32(0)
	if n := len(r.rows); n > 0 {
		last = r.rows[n-1].seq
	}

	added := make([]row, 0, len(seqs))
	appendOnly := true
	for _, seq := range seqs {
		if seq <= last {
			appendOnly = false
			break
		}
		if u := updates[seq]; u != nil {
			added = append(added, *u)
		}
	}

	if appendOnly {
		n := len(r.rows)
		if cap(r.rows) >= n+len(added) && r.frontier.CompareAndSwap(int64(n), int64(n+len(added))) {
			next.rows = append(r.rows[:n:n+len(added)], added...)
			next.frontier = r.frontier
		} else {
			next.rows = make([]row, n+len(added), (n+len(added))*5/4+1024)
			copy(next.rows, r.rows)
			copy(next.rows[n:], added)
			next.frontier = new(atomic.Int64)
			next.frontier.Store(int64(len(next.rows)))
			next.bytes += int64(cap(next.rows)-cap(r.rows)) * rowSliceBytes
		}

		return next, true
	}

	next.rows = make([]row, 0, len(r.rows)+len(seqs))
	i := 0
	for _, seq := range seqs {
		for i < len(r.rows) && r.rows[i].seq < seq {
			next.rows = append(next.rows, r.rows[i])
			i++
		}
		if i < len(r.rows) && r.rows[i].seq == seq {
			next.bytes -= int64(r.rows[i].size)
			i++
		}
		if u := updates[seq]; u != nil {
			next.rows = append(next.rows, *u)
		}
	}
	next.rows = append(next.rows, r.rows[i:]...)
	next.frontier = new(atomic.Int64)
	next.frontier.Store(int64(len(next.rows)))

	return next, true
}

func ResetQueryCache() {
	results.reset()
}

type QueryCacheStats struct {
	Entries int
	Bytes   int64
	Hits    int64
	Misses  int64
}

func QueryCache() QueryCacheStats {
	results.mu.Lock()
	n := len(results.items)
	results.mu.Unlock()

	return QueryCacheStats{Entries: n, Bytes: memory.HookResults.Used(), Hits: results.hits.Load(), Misses: results.misses.Load()}
}

func init() {
	memory.OnLevel(func(memory.Level) {
		results.shrink()
	})
}
