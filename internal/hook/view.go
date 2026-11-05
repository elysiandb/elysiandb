package hook

import (
	"errors"
	"hash/fnv"
	"maps"
	"math"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/dop251/goja"
	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/docpath"
	engine "github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/memory"
	"github.com/taymour/elysiandb/internal/storage"
)

const (
	viewShards        = 256
	viewChunkRows     = 1024
	viewRecentDeltas  = 1024
	foldCacheMinRows  = 256
	foldLaneMinRows   = 1 << 14
	maxFoldEntries    = 1 << 16
	maxViews          = 64
	viewIdleAfter     = 10 * time.Minute
	viewRefusedFor    = 30 * time.Second
	viewRefreshEvery  = 200 * time.Millisecond
	viewPartOverhead  = 160
	viewChunkOverhead = 80
	valBytes          = 16
	ownerNone         = uint32(0)
	ownerAll          = uint32(1)
	ownerIrregular    = uint32(2)
	ownerFirstPart    = uint32(3)
)

var (
	errViewRefused  = errors.New("view larger than its share of the memory budget")
	foldsDisabled   atomic.Bool
	errFoldNotReady = errors.New("view not ready")
)

func SetFoldsEnabled(enabled bool) {
	foldsDisabled.Store(!enabled)
}

type internTable struct {
	mu    sync.RWMutex
	ids   map[string]uint32
	strs  atomic.Pointer[[]string]
	bytes int64
}

func newInternTable() *internTable {
	t := &internTable{ids: map[string]uint32{"": 0}}
	first := []string{""}
	t.strs.Store(&first)

	return t
}

func (t *internTable) lookup(s string) (uint32, bool) {
	t.mu.RLock()
	id, ok := t.ids[s]
	t.mu.RUnlock()

	return id, ok
}

func (t *internTable) intern(s string) uint32 {
	t.mu.Lock()
	defer t.mu.Unlock()

	if id, ok := t.ids[s]; ok {
		return id
	}

	cur := append(*t.strs.Load(), s)
	id := uint32(len(cur) - 1)
	t.ids[s] = id
	t.strs.Store(&cur)
	t.bytes += int64(len(s)) + 64

	return id
}

func (t *internTable) count() int {
	return len(*t.strs.Load())
}

type viewChunk struct {
	seqs  []uint32
	cells []val
}

func (c *viewChunk) cost() int64 {
	return int64(cap(c.seqs))*4 + int64(cap(c.cells))*valBytes + viewChunkOverhead
}

type viewDelta struct {
	lsn    uint64
	seq    uint32
	old    []val
	new    []val
	hadOld bool
	hasNew bool
}

type viewPart struct {
	chunks  []*viewChunk
	rows    int
	version uint64
	covered uint64
	last    uint32
	recent  []viewDelta
}

type foldEntry struct {
	ver  uint64
	last uint32
	regs []val
}

type foldCache struct {
	mu    sync.Mutex
	items map[string]*foldEntry
	bytes int64
}

func newFoldCache() *foldCache {
	return &foldCache{items: map[string]*foldEntry{}}
}

func foldEntryCost(key string, regs []val) int64 {
	return int64(len(key)) + int64(len(regs))*valBytes + 128
}

func (f *foldCache) get(key string) *foldEntry {
	f.mu.Lock()
	defer f.mu.Unlock()

	return f.items[key]
}

func (f *foldCache) put(key string, e *foldEntry) int64 {
	f.mu.Lock()
	defer f.mu.Unlock()

	var delta int64
	if old, ok := f.items[key]; ok {
		delta -= foldEntryCost(key, old.regs)
	}

	if len(f.items) >= maxFoldEntries {
		dropped := 0
		for k, old := range f.items {
			delete(f.items, k)
			delta -= foldEntryCost(k, old.regs)
			dropped++
			if dropped >= maxFoldEntries/8 {
				break
			}
		}
	}

	f.items[key] = e
	delta += foldEntryCost(key, e.regs)
	f.bytes += delta

	return delta
}

type viewState struct {
	gen       uint64
	lsn       uint64
	shards    *[viewShards]map[string]*viewPart
	table     *internTable
	folds     *foldCache
	irregular int
	rows      int
}

func shardOf(key string) int {
	if key == "" {
		return 0
	}

	h := fnv.New32a()
	_, _ = h.Write([]byte(key))

	return int(h.Sum32() % viewShards)
}

func (s *viewState) part(key string) *viewPart {
	return s.shards[shardOf(key)][key]
}

type view struct {
	key       string
	entity    string
	base      map[string]map[string]string
	partField string
	fields    []string
	pinned    atomic.Bool
	used      atomic.Int64
	refused   atomic.Int64

	state atomic.Pointer[viewState]
	bytes atomic.Int64

	mu      sync.Mutex
	owner   []uint32
	counted int64

	flightMu sync.Mutex
	flight   chan struct{}
	err      error
}

type viewSet struct {
	mu     sync.Mutex
	engine *datastore.Engine
	items  map[string]*view
}

var views = &viewSet{items: map[string]*view{}}

func viewKey(entity string, base map[string]map[string]string, partField string, fields []string) string {
	return entity + "\x00" + filtersKey(base) + "\x00" + partField + "\x00" + strings.Join(fields, "\x00")
}

func (s *viewSet) get(e *datastore.Engine, entity string, base map[string]map[string]string, partField string, fields []string) *view {
	key := viewKey(entity, base, partField, fields)

	s.mu.Lock()
	defer s.mu.Unlock()

	if s.engine != e {
		s.dropAllLocked()
		s.engine = e
	}

	if v, ok := s.items[key]; ok {
		return v
	}

	if len(s.items) >= maxViews && !s.evictIdleLocked() {
		return nil
	}

	v := &view{key: key, entity: entity, base: base, partField: partField, fields: fields}
	s.items[key] = v

	return v
}

func (s *viewSet) dropAllLocked() {
	for _, v := range s.items {
		v.release()
	}
	s.items = map[string]*view{}
}

func (s *viewSet) evictIdleLocked() bool {
	var oldest *view
	for _, v := range s.items {
		if v.pinned.Load() {
			continue
		}
		if oldest == nil || v.used.Load() < oldest.used.Load() {
			oldest = v
		}
	}

	if oldest == nil {
		return false
	}

	oldest.release()
	delete(s.items, oldest.key)

	return true
}

func (s *viewSet) reset() {
	s.mu.Lock()
	defer s.mu.Unlock()

	for _, v := range s.items {
		v.release()
	}
}

func (s *viewSet) list() []*view {
	s.mu.Lock()
	defer s.mu.Unlock()

	out := make([]*view, 0, len(s.items))
	for _, v := range s.items {
		out = append(out, v)
	}

	return out
}

func (s *viewSet) sync(e *datastore.Engine, list []registeredHook) {
	want := map[string]bool{}
	for _, h := range list {
		if !h.hook.Enabled {
			continue
		}

		plan := planFor(h.hook.Script, scriptFunction(h.hook.Event))
		if plan == nil {
			continue
		}

		for _, def := range plan.folds {
			if !def.staticBase || engine.IsCoreEntityType(def.target) {
				continue
			}

			if v := s.get(e, def.target, def.baseFilters, def.partField, def.fields); v != nil {
				v.pinned.Store(true)
				want[v.key] = true
			}
		}
	}

	for _, v := range s.list() {
		if v.pinned.Load() && !want[v.key] {
			v.pinned.Store(false)
		}
	}
}

func scriptFunction(event string) string {
	if event == HookEventPreRead {
		return "preRead"
	}

	return "postRead"
}

func (s *viewSet) maintain(e *datastore.Engine, stop <-chan struct{}) {
	ticker := time.NewTicker(viewRefreshEvery)
	defer ticker.Stop()

	for {
		select {
		case <-stop:
			return
		case <-ticker.C:
		}

		cutoff := time.Now().Add(-viewIdleAfter).Unix()
		for _, v := range s.list() {
			if !v.pinned.Load() && v.used.Load() < cutoff {
				v.release()
				continue
			}

			if !v.pinned.Load() && v.state.Load() == nil {
				continue
			}

			if v.tryRefresh(e) != nil {
				continue
			}
		}

		results.sweep(e)
	}
}

func (v *view) release() {
	v.mu.Lock()
	defer v.mu.Unlock()

	v.state.Store(nil)
	v.owner = nil
	v.counted = 0
	memory.HookResults.Release(v.bytes.Swap(0))
}

func (v *view) tryRefresh(e *datastore.Engine) (err error) {
	defer func() {
		if r := recover(); r != nil {
			err = errFoldNotReady
		}
	}()

	if time.Now().Unix() < v.refused.Load() {
		return errViewRefused
	}

	snap := e.Snapshot(v.entity)
	if st := v.state.Load(); fresh(st, snap) {
		return nil
	}

	return v.advance(e)
}

func fresh(st *viewState, snap *datastore.Snapshot) bool {
	return st != nil && st.gen == snap.Generation() && st.lsn >= snap.Version()
}

func (v *view) current(e *datastore.Engine, guard *api_storage.Guard) (*viewState, bool, error) {
	if time.Now().Unix() < v.refused.Load() {
		return nil, false, errViewRefused
	}

	snap := e.Snapshot(v.entity)
	st := v.state.Load()
	if fresh(st, snap) {
		return st, false, nil
	}

	cold := st == nil
	timeout := api_storage.QueueTimeout()
	start := time.Now()
	for {
		v.flightMu.Lock()
		f := v.flight
		if f == nil {
			f = make(chan struct{})
			v.flight = f
			go func(done chan struct{}) {
				err := v.tryRefresh(e)
				v.flightMu.Lock()
				v.err = err
				v.flight = nil
				v.flightMu.Unlock()
				close(done)
			}(f)
		}
		v.flightMu.Unlock()

		ticker := time.NewTicker(20 * time.Millisecond)
	wait:
		for {
			select {
			case <-f:
				break wait
			case <-ticker.C:
				if err := guard.Check(); err != nil {
					ticker.Stop()
					return nil, cold, err
				}
				if timeout >= 0 && time.Since(start) >= timeout {
					ticker.Stop()
					return nil, cold, guard.Reject()
				}
			}
		}
		ticker.Stop()

		v.flightMu.Lock()
		err := v.err
		v.flightMu.Unlock()
		if err != nil {
			return nil, cold, err
		}

		st = v.state.Load()
		if fresh(st, snap) {
			return st, cold, nil
		}
	}
}

func (v *view) advance(e *datastore.Engine) error {
	v.mu.Lock()
	defer v.mu.Unlock()

	snap := e.Snapshot(v.entity)
	st := v.state.Load()
	if fresh(st, snap) {
		return nil
	}

	if st == nil || st.gen != snap.Generation() || st.table.count() > 4*st.rows+4096 {
		return v.rebuild(snap)
	}

	seqs, ok := snap.ChangedSince(st.lsn)
	if !ok {
		return v.rebuild(snap)
	}

	sort.Slice(seqs, func(i, j int) bool { return seqs[i] < seqs[j] })

	b := v.newBatch(st, snap.Version(), snap.Generation())
	for _, seq := range seqs {
		d, ok := snap.BySeq(seq)
		if !ok {
			b.apply(seq, nil, false)
			continue
		}

		raw, err := d.Bytes()
		if err != nil {
			return err
		}

		doc, err := datastore.DecodeMap(raw)
		if err != nil || doc == nil {
			b.apply(seq, nil, false)
			continue
		}

		b.apply(seq, doc, true)
	}

	return v.publish(b)
}

func (v *view) rebuild(snap *datastore.Snapshot) error {
	v.owner = make([]uint32, 0, snap.NextSeq())
	v.counted = 0

	empty := &viewState{shards: &[viewShards]map[string]*viewPart{}, table: newInternTable(), folds: newFoldCache()}
	b := v.newBatch(empty, snap.Version(), snap.Generation())
	b.rebuilding = true

	var failed error
	snap.ScanValues(func(d datastore.Doc, raw []byte, err error) bool {
		if err != nil {
			failed = err
			return false
		}

		doc, err := datastore.DecodeMap(raw)
		if err != nil || doc == nil {
			return true
		}

		b.apply(d.Seq(), doc, true)

		return true
	})

	if failed != nil {
		return failed
	}

	return v.publish(b)
}

func (v *view) publish(b *viewBatch) error {
	b.delta += b.next.table.bytes - v.counted + int64(cap(v.owner)-b.ownerCap)*4
	v.counted = b.next.table.bytes

	if limit := memory.HookResults.Effective(); memory.Budget() > 0 && limit > 0 {
		held := b.delta
		if !b.rebuilding {
			held += v.bytes.Load()
		}
		if held > limit/2 {
			v.state.Store(nil)
			v.owner = nil
			v.counted = 0
			memory.HookResults.Release(v.bytes.Swap(0))
			v.refused.Store(time.Now().Add(viewRefusedFor).Unix())
			memory.HookResults.Refuse()
			return errViewRefused
		}
	}

	if b.rebuilding {
		memory.HookResults.Release(v.bytes.Swap(0))
	}

	v.bytes.Add(b.delta)
	memory.HookResults.Add(b.delta)
	v.state.Store(b.next)

	return nil
}

type viewBatch struct {
	v          *view
	next       *viewState
	lsn        uint64
	width      int
	cellsBuf   []val
	ownedShard [viewShards]bool
	ownedPart  map[*viewPart]bool
	ownedChunk map[*viewChunk]bool
	delta      int64
	ownerCap   int
	rebuilding bool
}

func (v *view) newBatch(st *viewState, lsn, gen uint64) *viewBatch {
	shards := *st.shards
	next := &viewState{gen: gen, lsn: lsn, shards: &shards, table: st.table, folds: st.folds, irregular: st.irregular, rows: st.rows}

	return &viewBatch{v: v, next: next, lsn: lsn, width: len(v.fields), ownerCap: cap(v.owner), ownedPart: map[*viewPart]bool{}, ownedChunk: map[*viewChunk]bool{}}
}

func (b *viewBatch) ownerOf(seq uint32) uint32 {
	if int(seq) < len(b.v.owner) {
		return b.v.owner[seq]
	}

	return ownerNone
}

func (b *viewBatch) setOwner(seq, owner uint32) {
	if int(seq) >= len(b.v.owner) {
		if owner == ownerNone {
			return
		}
		if int(seq) < cap(b.v.owner) {
			b.v.owner = b.v.owner[:seq+1]
		} else {
			grown := make([]uint32, seq+1, max(int(seq)+1, 2*cap(b.v.owner)))
			copy(grown, b.v.owner)
			b.v.owner = grown
		}
	}

	b.v.owner[seq] = owner
}

func (b *viewBatch) partKey(owner uint32) string {
	if owner == ownerAll {
		return ""
	}

	return (*b.next.table.strs.Load())[owner-ownerFirstPart]
}

func (b *viewBatch) cellsOf(doc map[string]any) []val {
	cells := make([]val, b.width)
	for i, f := range b.v.fields {
		raw, ok := doc[f]
		if !ok {
			continue
		}

		switch t := raw.(type) {
		case nil:
			cells[i] = nullVal
		case string:
			cells[i] = val{k: vStr, id: b.next.table.intern(t)}
		case float64:
			cells[i] = numVal(t)
		case bool:
			cells[i] = boolVal(t)
		default:
			cells[i] = val{k: vOpaque}
		}
	}

	return cells
}

func (b *viewBatch) classify(doc map[string]any) (uint32, bool) {
	if !api_storage.FiltersMatchEntity(doc, b.v.base) {
		return ownerNone, false
	}

	if b.v.partField == "" {
		return ownerAll, true
	}

	raw, ok := doc[b.v.partField]
	if !ok || raw == nil {
		return ownerNone, false
	}

	s, ok := raw.(string)
	if !ok {
		return ownerIrregular, false
	}

	return b.next.table.intern(strings.ToLower(s)) + ownerFirstPart, true
}

func (b *viewBatch) apply(seq uint32, doc map[string]any, exists bool) {
	before := b.ownerOf(seq)

	after := ownerNone
	var cells []val
	if exists {
		owner, inView := b.classify(doc)
		after = owner
		if inView {
			cells = b.cellsOf(doc)
		}
	}

	if before == ownerIrregular && after != ownerIrregular {
		b.next.irregular--
	}
	if after == ownerIrregular && before != ownerIrregular {
		b.next.irregular++
	}

	if before >= ownerAll && before != ownerIrregular {
		if before == after {
			p := b.part(b.partKey(before))
			old := b.update(p, seq, cells)
			b.record(p, viewDelta{seq: seq, old: old, new: cells, hadOld: true, hasNew: true})
			b.setOwner(seq, after)
			return
		}

		p := b.part(b.partKey(before))
		old := b.remove(p, seq)
		b.record(p, viewDelta{seq: seq, old: old, hadOld: true})
		if p.rows == 0 {
			b.dropPart(b.partKey(before))
		}
	}

	if after >= ownerAll && after != ownerIrregular {
		p := b.part(b.partKey(after))
		b.insert(p, seq, cells)
		b.record(p, viewDelta{seq: seq, new: cells, hasNew: true})
	}

	b.setOwner(seq, after)
}

func (b *viewBatch) ownShard(i int) map[string]*viewPart {
	if !b.ownedShard[i] {
		b.ownedShard[i] = true
		if b.next.shards[i] == nil {
			b.next.shards[i] = map[string]*viewPart{}
		} else {
			b.next.shards[i] = maps.Clone(b.next.shards[i])
		}
	}

	return b.next.shards[i]
}

func (b *viewBatch) part(key string) *viewPart {
	shard := b.ownShard(shardOf(key))
	p := shard[key]
	if p == nil {
		p = &viewPart{covered: b.lsn, version: b.lsn}
		shard[key] = p
		b.ownedPart[p] = true
		b.delta += viewPartOverhead + int64(len(key))
		return p
	}

	if !b.ownedPart[p] {
		c := *p
		c.chunks = append(make([]*viewChunk, 0, len(p.chunks)+1), p.chunks...)
		p = &c
		shard[key] = p
		b.ownedPart[p] = true
	}

	return p
}

func (b *viewBatch) dropPart(key string) {
	shard := b.ownShard(shardOf(key))
	if _, ok := shard[key]; ok {
		delete(shard, key)
		b.delta -= viewPartOverhead + int64(len(key))
	}
}

func (b *viewBatch) own(p *viewPart, i int) *viewChunk {
	c := p.chunks[i]
	if b.ownedChunk[c] {
		return c
	}

	room := min(viewChunkRows, 2*len(c.seqs)+4)
	n := &viewChunk{
		seqs:  append(make([]uint32, 0, room), c.seqs...),
		cells: append(make([]val, 0, room*b.width), c.cells...),
	}
	b.delta += n.cost() - c.cost()
	p.chunks[i] = n
	b.ownedChunk[n] = true

	return n
}

func (b *viewBatch) find(p *viewPart, seq uint32) (int, int, bool) {
	ci := sort.Search(len(p.chunks), func(i int) bool {
		s := p.chunks[i].seqs
		return s[len(s)-1] >= seq
	})
	if ci == len(p.chunks) {
		return ci, 0, false
	}

	s := p.chunks[ci].seqs
	ri := sort.Search(len(s), func(i int) bool { return s[i] >= seq })

	return ci, ri, ri < len(s) && s[ri] == seq
}

func (b *viewBatch) insert(p *viewPart, seq uint32, cells []val) {
	p.rows++
	b.next.rows++
	if len(p.chunks) == 0 || seq > p.last {
		if seq > p.last || p.rows == 1 {
			p.last = seq
		}

		if n := len(p.chunks); n > 0 && len(p.chunks[n-1].seqs) < viewChunkRows {
			c := b.own(p, n-1)
			grown := cap(c.seqs)
			c.seqs = append(c.seqs, seq)
			c.cells = append(c.cells, cells...)
			if cap(c.seqs) != grown {
				b.delta += int64(cap(c.seqs)-grown) * (4 + int64(b.width)*valBytes)
			}
			return
		}

		c := &viewChunk{seqs: make([]uint32, 1, 4), cells: make([]val, b.width, 4*max(b.width, 1))}
		c.seqs[0] = seq
		copy(c.cells, cells)
		p.chunks = append(p.chunks, c)
		b.ownedChunk[c] = true
		b.delta += c.cost()
		return
	}

	ci, ri, _ := b.find(p, seq)
	if ci == len(p.chunks) {
		ci = len(p.chunks) - 1
		ri = len(p.chunks[ci].seqs)
	}

	c := b.own(p, ci)
	before := c.cost()
	c.seqs = append(c.seqs, 0)
	copy(c.seqs[ri+1:], c.seqs[ri:])
	c.seqs[ri] = seq
	c.cells = append(c.cells, make([]val, b.width)...)
	copy(c.cells[(ri+1)*b.width:], c.cells[ri*b.width:])
	copy(c.cells[ri*b.width:], cells)

	if len(c.seqs) > viewChunkRows {
		mid := len(c.seqs) / 2
		right := &viewChunk{
			seqs:  append(make([]uint32, 0, viewChunkRows), c.seqs[mid:]...),
			cells: append(make([]val, 0, viewChunkRows*b.width), c.cells[mid*b.width:]...),
		}
		c.seqs = c.seqs[:mid]
		c.cells = c.cells[:mid*b.width]
		p.chunks = append(p.chunks, nil)
		copy(p.chunks[ci+2:], p.chunks[ci+1:])
		p.chunks[ci+1] = right
		b.ownedChunk[right] = true
		b.delta += right.cost()
	}
	b.delta += c.cost() - before
}

func (b *viewBatch) update(p *viewPart, seq uint32, cells []val) []val {
	ci, ri, ok := b.find(p, seq)
	if !ok {
		return nil
	}

	c := b.own(p, ci)
	row := c.cells[ri*b.width : (ri+1)*b.width]
	old := append([]val(nil), row...)
	copy(row, cells)

	return old
}

func (b *viewBatch) remove(p *viewPart, seq uint32) []val {
	ci, ri, ok := b.find(p, seq)
	if !ok {
		return nil
	}

	c := b.own(p, ci)
	old := append([]val(nil), c.cells[ri*b.width:(ri+1)*b.width]...)
	c.seqs = append(c.seqs[:ri], c.seqs[ri+1:]...)
	c.cells = append(c.cells[:ri*b.width], c.cells[(ri+1)*b.width:]...)
	p.rows--
	b.next.rows--

	if len(c.seqs) == 0 {
		b.delta -= c.cost()
		p.chunks = append(p.chunks[:ci], p.chunks[ci+1:]...)
	}

	if n := len(p.chunks); n > 0 {
		s := p.chunks[n-1].seqs
		p.last = s[len(s)-1]
	} else {
		p.last = 0
	}

	return old
}

func (b *viewBatch) record(p *viewPart, d viewDelta) {
	p.version = b.lsn
	if b.rebuilding {
		p.covered = b.lsn
		return
	}

	if p.rows < foldCacheMinRows && len(p.recent) == 0 {
		p.covered = b.lsn
		return
	}

	d.lsn = b.lsn

	if len(p.recent) >= viewRecentDeltas || cap(p.recent) == len(p.recent) {
		keep := p.recent
		if len(keep) >= viewRecentDeltas {
			p.covered = keep[len(keep)-viewRecentDeltas/2-1].lsn
			keep = keep[len(keep)-viewRecentDeltas/2:]
		}
		p.recent = append(make([]viewDelta, 0, min(viewRecentDeltas, 2*len(keep)+8)), keep...)
	}
	p.recent = append(p.recent, d)
}

func (v *view) strVal(table *internTable, fr *frame, s string) val {
	if id, ok := table.lookup(s); ok {
		return val{k: vStr, id: id}
	}

	fr.tmp = append(fr.tmp, s)

	return val{k: vTmp, id: uint32(len(fr.tmp) - 1)}
}

func (v *view) toVal(table *internTable, fr *frame, x goja.Value) (val, bool) {
	if x == nil || goja.IsUndefined(x) {
		return undefVal, true
	}

	if goja.IsNull(x) {
		return nullVal, true
	}

	switch t := x.Export().(type) {
	case bool:
		return boolVal(t), true
	case int64:
		return numVal(float64(t)), true
	case float64:
		return numVal(t), true
	case string:
		return v.strVal(table, fr, t), true
	}

	return undefVal, false
}

func appendValKey(b []byte, fr *frame, x val) []byte {
	b = append(b, byte(x.k))
	switch x.k {
	case vNum, vBool:
		bits := math.Float64bits(x.n)
		for i := 0; i < 8; i++ {
			b = append(b, byte(bits>>(8*i)))
		}
	case vStr, vTmp:
		s := fr.str(x)
		l := len(s)
		b = append(b, byte(l), byte(l>>8), byte(l>>16), byte(l>>24))
		b = append(b, s...)
	}

	return b
}

func (def *foldDef) runRow(fr *frame, row []val, base int) bool {
	fr.row = row
	fr.tmp = fr.tmp[:base]
	r := def.body(fr)

	return r != ctlBail && !fr.bail
}

func (def *foldDef) inert(fr *frame, row []val, present bool, base int) (bool, bool) {
	if !present {
		return true, true
	}

	for i := range fr.regs {
		fr.regs[i] = numVal(0)
	}
	fr.touched = false
	if !def.runRow(fr, row, base) {
		return false, false
	}

	return !fr.touched, true
}

func (def *foldDef) fullPass(fr *frame, part *viewPart, base int, width int, check func() error) (bool, error) {
	for _, c := range part.chunks {
		if err := check(); err != nil {
			return false, err
		}

		for i := range c.seqs {
			if !def.runRow(fr, c.cells[i*width:(i+1)*width], base) {
				return false, nil
			}
		}
	}

	return true, nil
}

func cacheableRegs(regs []val) bool {
	for _, r := range regs {
		if r.k == vTmp || r.k == vArr || r.k == vOpaque {
			return false
		}
	}

	return true
}

func (def *foldDef) incremental(fr *frame, e *foldEntry, part *viewPart, base int) ([]val, uint32, bool) {
	if e.ver < part.covered {
		return nil, 0, false
	}

	i := sort.Search(len(part.recent), func(i int) bool { return part.recent[i].lsn > e.ver })
	pending := part.recent[i:]
	regs := append([]val(nil), e.regs...)
	last := e.last
	for _, d := range pending {
		if !d.hadOld && d.seq > last {
			copy(fr.regs, regs)
			fr.touched = false
			if !def.runRow(fr, d.new, base) {
				return nil, 0, false
			}
			copy(regs, fr.regs)
			last = d.seq
			continue
		}

		if !def.accumulating {
			return nil, 0, false
		}

		oldInert, ok1 := def.inert(fr, d.old, d.hadOld, base)
		newInert, ok2 := def.inert(fr, d.new, d.hasNew, base)
		if !ok1 || !ok2 || !oldInert || !newInert {
			return nil, 0, false
		}
	}

	return regs, last, true
}

func (def *foldDef) evaluate(v *view, st *viewState, partKey string, regsIn, freeIn []goja.Value, lane func() (func(), error), check func() error) ([]val, []string, bool, error) {
	strs := *st.table.strs.Load()
	fr := acquireFrame(len(def.regs), def.locals, len(def.lits), len(def.frees), strs)
	defer releaseFrame(fr)

	for i, s := range def.lits {
		fr.lits[i] = v.strVal(st.table, fr, s)
	}

	for i, x := range regsIn {
		c, ok := v.toVal(st.table, fr, x)
		if !ok {
			return nil, nil, false, nil
		}
		fr.regs[i] = c
	}

	for i, x := range freeIn {
		c, ok := v.toVal(st.table, fr, x)
		if !ok {
			return nil, nil, false, nil
		}
		fr.free[i] = c
	}

	base := len(fr.tmp)
	width := len(def.fields)
	part := st.part(partKey)

	output := func() ([]val, []string, bool, error) {
		out := append([]val(nil), fr.regs...)
		names := make([]string, len(out))
		for i, r := range out {
			if isString(r) {
				names[i] = fr.str(r)
			}
		}
		return out, names, true, nil
	}

	if part == nil || part.rows == 0 {
		return output()
	}

	if part.rows < foldCacheMinRows {
		foldCounters.cached.Add(1)
		ok, err := def.fullPass(fr, part, base, width, check)
		if err != nil || !ok {
			return nil, nil, false, err
		}
		fr.tmp = fr.tmp[:base]
		return output()
	}

	keyBuf := make([]byte, 0, 96)
	keyBuf = append(keyBuf, def.id...)
	keyBuf = append(keyBuf, partKey...)
	keyBuf = append(keyBuf, 0)
	for _, r := range fr.regs {
		keyBuf = appendValKey(keyBuf, fr, r)
	}
	for _, f := range fr.free {
		keyBuf = appendValKey(keyBuf, fr, f)
	}
	key := string(keyBuf)

	initial := append([]val(nil), fr.regs...)
	e := st.folds.get(key)
	if e != nil && e.ver == part.version {
		foldCounters.cached.Add(1)
		copy(fr.regs, e.regs)
		return output()
	}

	if e != nil && e.ver < part.version {
		if regs, last, ok := def.incremental(fr, e, part, base); ok {
			foldCounters.incremental.Add(1)
			copy(fr.regs, regs)
			fr.tmp = fr.tmp[:base]
			if cacheableRegs(regs) {
				v.addCost(st.folds.put(key, &foldEntry{ver: part.version, last: last, regs: regs}))
			}
			return output()
		}
		fr.bail = false
	}

	if part.rows >= foldLaneMinRows {
		release, err := lane()
		if err != nil {
			return nil, nil, false, err
		}
		defer release()
	}

	foldCounters.full.Add(1)
	copy(fr.regs, initial)
	ok, err := def.fullPass(fr, part, base, width, check)
	if err != nil || !ok {
		return nil, nil, false, err
	}
	fr.tmp = fr.tmp[:base]

	if cacheableRegs(fr.regs) && (e == nil || e.ver < part.version) {
		v.addCost(st.folds.put(key, &foldEntry{ver: part.version, last: part.last, regs: append([]val(nil), fr.regs...)}))
	}

	return output()
}

func (v *view) addCost(delta int64) {
	v.bytes.Add(delta)
	memory.HookResults.Add(delta)
}

func partitionValue(raw string) (string, bool) {
	if strings.ContainsAny(raw, "*\\") {
		return "", false
	}

	if _, ok, _ := docpath.ParseDate(raw); ok {
		return "", false
	}

	return strings.ToLower(raw), true
}

type viewToken struct {
	target  string
	filters map[string]map[string]string
	core    bool
	key     string
}

func (r *Request) pinnedView(v *view, e *datastore.Engine) (*viewState, bool, error) {
	r.mu.Lock()
	st, ok := r.pins[v]
	r.mu.Unlock()
	if ok {
		return st, false, nil
	}

	st, cold, err := v.current(e, r.guard)
	if err != nil {
		return nil, cold, err
	}

	r.mu.Lock()
	if cur, ok := r.pins[v]; ok {
		st = cur
	} else {
		if r.pins == nil {
			r.pins = map[*view]*viewState{}
		}
		r.pins[v] = st
	}
	r.mu.Unlock()

	return st, cold, nil
}

func (r *Request) fold(h Hook, def *foldDef, tok *viewToken, regs, free []goja.Value) ([]val, []string, error) {
	out, names, err := r.foldOnView(h, def, tok, regs, free)
	if out == nil && err == nil {
		foldCounters.fallbacks.Add(1)
	}

	return out, names, err
}

func (r *Request) foldOnView(h Hook, def *foldDef, tok *viewToken, regs, free []goja.Value) ([]val, []string, error) {
	if foldsDisabled.Load() || tok.core || !engine.IsEngineInternal() {
		return nil, nil, nil
	}

	e := storage.Engine()
	if e == nil {
		return nil, nil, nil
	}

	if !h.ByPassACL && acl.ReadModeFor(r.username, tok.target) != acl.ReadAll {
		return nil, nil, nil
	}

	partKey := ""
	base := tok.filters
	if def.partField != "" {
		cond, ok := tok.filters[def.partField]
		if !ok || len(cond) != 1 {
			return nil, nil, nil
		}

		partKey, ok = partitionValue(cond["eq"])
		if !ok {
			return nil, nil, nil
		}

		base = make(map[string]map[string]string, len(tok.filters))
		for f, ops := range tok.filters {
			if f != def.partField {
				base[f] = ops
			}
		}
	}

	if api_storage.ExtractAutoIncludes(base) != "" {
		return nil, nil, nil
	}

	v := views.get(e, tok.target, base, def.partField, def.fields)
	if v == nil {
		return nil, nil, nil
	}
	v.used.Store(time.Now().Unix())

	start := time.Now()
	st, cold, err := r.pinnedView(v, e)
	if errors.Is(err, errViewRefused) || errors.Is(err, errFoldNotReady) {
		return nil, nil, nil
	}
	if err != nil {
		if errors.Is(err, api_storage.ErrBusy) || errors.Is(err, api_storage.ErrAbandoned) || errors.Is(err, api_storage.ErrDeadline) {
			return nil, nil, err
		}
		return nil, nil, nil
	}

	if st.irregular > 0 {
		return nil, nil, nil
	}

	patient := r.guard.Until(time.Time{})
	lane := func() (func(), error) { return api_storage.AcquireHookLane(patient) }
	check := func() error { return r.guard.Check() }

	out, names, ok, err := def.evaluate(v, st, partKey, regs, free, lane, check)
	if err != nil {
		return nil, nil, err
	}
	if !ok {
		return nil, nil, nil
	}

	if cold {
		results.misses.Add(1)
	} else {
		results.hits.Add(1)
	}

	rows := 0
	if p := st.part(partKey); p != nil {
		rows = p.rows
	}
	observeFold(h, rows, !cold, start)

	return out, names, nil
}

type FoldStats struct {
	Cached      int64
	Incremental int64
	FullPasses  int64
	Fallbacks   int64
	Views       int
	Bytes       int64
}

var foldCounters struct {
	cached      atomic.Int64
	incremental atomic.Int64
	full        atomic.Int64
	fallbacks   atomic.Int64
}

func Folds() FoldStats {
	list := views.list()
	var bytes int64
	for _, v := range list {
		bytes += v.bytes.Load()
	}

	return FoldStats{
		Cached:      foldCounters.cached.Load(),
		Incremental: foldCounters.incremental.Load(),
		FullPasses:  foldCounters.full.Load(),
		Fallbacks:   foldCounters.fallbacks.Load(),
		Views:       len(list),
		Bytes:       bytes,
	}
}
