package hook

import (
	"container/list"
	"errors"
	"github.com/taymour/elysiandb/internal/log"
	"runtime"
	"sort"
	"strings"
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
	refused map[string]time.Time
	hits    atomic.Int64
	misses  atomic.Int64
}

var results = newResultCache()

func newResultCache() *resultCache {
	return &resultCache{items: map[string]*cacheItem{}, lru: list.New(), flights: map[string]*flight{}, refused: map[string]time.Time{}}
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

func (c *resultCache) store(key string, res *result) bool {
	limit := memory.HookResults.Effective()
	if limit > 0 && res.bytes > limit {
		return false
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

	return true
}

func (c *resultCache) evictLocked(limit int64) {
	for limit > 0 && memory.HookResults.Used() > limit && c.lru.Len() > 1 {
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
	c.refused = map[string]time.Time{}
	c.lru.Init()
}

func (c *resultCache) get(key, version string, guard *api_storage.Guard, build func(prev *result) (*result, error)) (*result, bool, error) {
	if version == "" {
		res, err := build(nil)
		return res, false, err
	}

	missed := false
	for {
		if res, _ := c.lookup(key, version); res != nil {
			if !missed {
				c.hits.Add(1)
			}
			return res, !missed, nil
		}

		if !missed {
			c.misses.Add(1)
			missed = true
		}

		c.mu.Lock()
		if until, ok := c.refused[key]; ok {
			if time.Now().Before(until) {
				c.mu.Unlock()
				return nil, false, guard.Reject()
			}
			delete(c.refused, key)
		}

		f, busy := c.flights[key]
		if !busy {
			f = &flight{done: make(chan struct{})}
			c.flights[key] = f
			c.mu.Unlock()

			go c.fill(key, version, f, build)

			res, _, err := waitFlight(f, guard)
			return res, false, err
		}
		c.mu.Unlock()

		res, _, err := waitFlight(f, guard)
		if err != nil {
			return nil, false, err
		}

		if res != nil && res.version == version {
			return res, false, nil
		}
	}
}

func (c *resultCache) fill(key, version string, f *flight, build func(prev *result) (*result, error)) {
	c.mu.Lock()
	var prev *result
	if it, ok := c.items[key]; ok {
		prev = it.res
	}
	c.mu.Unlock()

	f.res, f.err = build(prev)
	if f.err == nil {
		f.res.version = version
		if !c.store(key, f.res) && memory.Budget() > 0 {
			memory.HookResults.Refuse()
			f.res, f.err = nil, api_storage.ErrBusy
		}
	}

	c.mu.Lock()
	delete(c.flights, key)
	if errors.Is(f.err, api_storage.ErrBusy) {
		c.refused[key] = time.Now().Add(refusedFor)
		entity, _, _ := strings.Cut(key, "\x00")
		if suppressed, ok := oversized.Allow(entity); ok {
			hookLog.Warn("hook query result larger than its share of the memory budget, refused", log.WithSuppressed([]any{"entity", entity, "share_mb", memory.HookResults.Effective() >> 20}, suppressed)...)
		}
	}
	c.mu.Unlock()
	close(f.done)
}

const refusedFor = 30 * time.Second

var oversized = log.NewThrottle(time.Minute)

func waitFlight(f *flight, guard *api_storage.Guard) (*result, bool, error) {
	ticker := time.NewTicker(20 * time.Millisecond)
	defer ticker.Stop()

	timeout := api_storage.QueueTimeout()
	start := time.Now()
	for {
		select {
		case <-f.done:
			return f.res, true, f.err
		case <-ticker.C:
			if err := guard.Check(); err != nil {
				return nil, false, err
			}
			if timeout >= 0 && time.Since(start) >= timeout {
				return nil, false, guard.Reject()
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

	rows, built, err := buildRows(planned.Docs, prev, keep)
	if err != nil {
		return nil, err
	}

	res.rows = rows
	b.bytes += built
	for i := 1; i < len(rows); i++ {
		if rows[i].seq <= rows[i-1].seq {
			res.bySeq = false
			break
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

const (
	parallelBuildRows = 8192
	buildCheckEvery   = 1024
)

func buildRows(docs []datastore.Doc, prev *result, keep func(map[string]any) bool) ([]row, int64, error) {
	workers := 1
	if len(docs) >= parallelBuildRows {
		workers = max(1, min(4, runtime.GOMAXPROCS(0)/2))
	}

	parts := make([][]row, workers)
	sizes := make([]int64, workers)
	var total atomic.Int64
	var refused atomic.Bool
	per := (len(docs) + workers - 1) / workers

	var wg sync.WaitGroup
	for w := 0; w < workers; w++ {
		lo, hi := w*per, min((w+1)*per, len(docs))
		if lo >= hi {
			continue
		}

		wg.Add(1)
		go func(w, lo, hi int) {
			defer wg.Done()

			b := newRowBuilder()
			out := make([]row, 0, hi-lo)
			for i := lo; i < hi && !refused.Load(); i++ {
				d := docs[i]
				seq, lsn := d.Seq(), d.LSN()
				if r, ok := prev.reusable(seq, lsn); ok {
					out = append(out, r)
					b.bytes += int64(r.size)
				} else if raw, err := d.Bytes(); err == nil {
					if doc, err := datastore.DecodeMap(raw); err == nil && doc != nil && keep(doc) {
						out = append(out, b.build(doc, seq, lsn, len(raw)))
					}
				}

				if (i-lo)%buildCheckEvery == buildCheckEvery-1 {
					grown := total.Add(b.bytes-sizes[w]) + int64(len(docs))*rowSliceBytes
					sizes[w] = b.bytes
					if limit := memory.HookResults.Effective(); memory.Budget() > 0 && limit > 0 && grown > limit {
						refused.Store(true)
					}
				}
			}

			sizes[w] = b.bytes
			parts[w] = out
		}(w, lo, hi)
	}
	wg.Wait()

	if refused.Load() {
		memory.HookResults.Refuse()
		return nil, 0, api_storage.ErrBusy
	}

	n := 0
	for _, p := range parts {
		n += len(p)
	}

	rows := make([]row, 0, n)
	var built int64
	for w, p := range parts {
		rows = append(rows, p...)
		built += sizes[w]
	}

	return rows, built, nil
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
