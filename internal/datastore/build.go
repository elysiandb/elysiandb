package datastore

import (
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/memory"
	"github.com/taymour/elysiandb/internal/pagetree"
)

const (
	smallEntityDocs   = 512
	maxBatchEntries   = 2_000_000
	maxBatchIndexes   = 32
	buildEntryBytes   = 72
	searchDocBytes    = 640
	smallBuildBytes   = 1 << 20
	IndexIdleAfter    = time.Hour
	idleSweepInterval = time.Minute
)

type usageClock struct {
	at atomic.Int64
}

func (ent *entity) touch(name string) {
	now := time.Now().Unix()
	if v, ok := ent.usage.Load(name); ok {
		last := v.(*usageClock)
		if last.at.Load() != now {
			last.at.Store(now)
		}
		return
	}

	c := &usageClock{}
	c.at.Store(now)
	ent.usage.LoadOrStore(name, c)
}

func (ent *entity) lastUse(name string) time.Time {
	if v, ok := ent.usage.Load(name); ok {
		return time.Unix(v.(*usageClock).at.Load(), 0)
	}

	return time.Time{}
}

func (e *Engine) schedule(ent *entity) bool {
	if !e.opts.Background || e.closed.Load() {
		return false
	}

	if !ent.queued.CompareAndSwap(false, true) {
		return true
	}

	select {
	case e.builds <- ent:
		return true
	default:
		ent.queued.Store(false)
		return false
	}
}

func (e *Engine) buildLoop() {
	defer e.wg.Done()

	sem := make(chan struct{}, e.opts.IndexWorkers)
	var running sync.WaitGroup
	for {
		select {
		case <-e.stop:
			running.Wait()
			return
		case ent := <-e.builds:
			sem <- struct{}{}
			running.Add(1)
			go func() {
				defer func() {
					<-sem
					running.Done()
				}()
				e.buildEntity(ent, true)
			}()
		}
	}
}

type claimedBuild struct {
	name string
	spec IndexSpec
	st   *buildState
}

func (e *Engine) buildEntity(ent *entity, background bool) {
	if background {
		ent.queued.Store(false)
	}

	for {
		more := e.buildPass(ent)
		if !more {
			return
		}

		if background && e.schedule(ent) {
			return
		}
	}
}

func (e *Engine) claim(ent *entity) (*root, *buildPass, []claimedBuild, bool) {
	ent.apply.Lock()
	defer ent.apply.Unlock()

	snap := ent.root.Load()
	var pending []claimedBuild
	for name, st := range ent.building {
		if st.pass != nil {
			continue
		}

		idx, ok := snap.indexes[name]
		if !ok || idx.ready || idx.dormant {
			delete(ent.building, name)
			continue
		}

		pending = append(pending, claimedBuild{name: name, spec: idx.spec, st: st})
	}

	sort.Slice(pending, func(i, j int) bool { return pending[i].st.order < pending[j].st.order })

	limit := min(maxBatchIndexes, max(1, maxBatchEntries/max(1, snap.slots.Len())))
	if budget := memory.Builds.Limit(); budget > 0 {
		limit = min(limit, max(1, int(budget/int64(max(1, snap.slots.Len())*buildEntryBytes))))
	}
	more := len(pending) > limit
	if more {
		pending = pending[:limit]
	}

	if len(pending) == 0 {
		return nil, nil, nil, false
	}

	pass := &buildPass{before: map[uint32]map[string]any{}}
	for _, c := range pending {
		c.st.pass = pass
	}
	ent.passes[pass] = struct{}{}

	return snap, pass, pending, more
}

func (e *Engine) buildPass(ent *entity) bool {
	snap, pass, jobs, more := e.claim(ent)
	if pass == nil {
		return false
	}

	start := time.Now()
	keys := make([][]string, len(jobs))
	searchJob := -1
	for i, j := range jobs {
		if j.spec.Kind == IndexSearch {
			searchJob = i
		}
	}

	need := int64(snap.slots.Len()) * int64(len(jobs)) * buildEntryBytes
	if searchJob >= 0 {
		need += int64(snap.slots.Len()) * searchDocBytes
	}

	if need >= smallBuildBytes {
		if err := memory.Builds.Acquire(need, -1, func() error {
			if e.closed.Load() {
				return ErrClosed
			}
			return nil
		}); err != nil {
			e.abandonPass(ent, pass, jobs)
			return false
		}
		defer memory.Builds.Release(need)
	}

	docs := 0
	each := func(fn func(seq uint32, doc map[string]any)) {
		snap.slots.AscendAt(0, func(k string, v []byte) bool {
			s := decodeSlotValue(keySeq(k), v, true)
			fn(s.seq, e.decodeSlot(s))
			return true
		})
	}

	var searchKeys []string
	var searchVals [][]byte
	var containers map[string][]byte
	if searchJob >= 0 {
		containers = map[string][]byte{}
	}

	bitmaps := map[int]map[string][]byte{}
	for i, j := range jobs {
		if j.spec.Kind == IndexBitmap {
			bitmaps[i] = map[string][]byte{}
		}
	}

	each(func(seq uint32, doc map[string]any) {
		for i, j := range jobs {
			if i == searchJob {
				continue
			}
			if m := bitmaps[i]; m != nil {
				chunk, off := seq>>postingChunkBits, uint16(seq&(postingChunk-1))
				for _, k := range uniqueKeys(extractKeys(j.spec, doc)) {
					key := bitmapKey(k, chunk)
					m[key] = postingAdd(m[key], off)
				}
				continue
			}
			for _, k := range uniqueKeys(extractKeys(j.spec, doc)) {
				keys[i] = append(keys[i], k+seqKey(seq))
			}
		}

		if containers != nil && doc != nil {
			chunk := seq >> postingChunkBits
			off := uint16(seq & (postingChunk - 1))
			for _, g := range searchGrams(doc) {
				key := postingKey(g, chunk)
				containers[key] = postingAdd(containers[key], off)
			}
		}

		docs++
	})

	if containers != nil {
		searchKeys = make([]string, 0, len(containers))
		for k := range containers {
			searchKeys = append(searchKeys, k)
		}
		sort.Strings(searchKeys)
		searchVals = make([][]byte, len(searchKeys))
		for i, k := range searchKeys {
			searchVals[i] = containers[k]
		}
		containers = nil
	}

	trees := make([]pagetree.Tree, len(jobs))
	for i := range jobs {
		if i == searchJob {
			trees[i] = pagetree.FromSorted(e.pages, searchKeys, searchVals)
			continue
		}
		if m := bitmaps[i]; m != nil {
			bk := make([]string, 0, len(m))
			for k := range m {
				bk = append(bk, k)
			}
			sort.Strings(bk)
			bv := make([][]byte, len(bk))
			for n, k := range bk {
				bv[n] = m[k]
			}
			trees[i] = pagetree.FromSorted(e.pages, bk, bv)
			bitmaps[i] = nil
			continue
		}
		sort.Strings(keys[i])
		trees[i] = pagetree.FromSorted(e.pages, keys[i], nil)
		keys[i] = nil
	}

	requeue := e.commitPass(ent, snap, pass, jobs, trees)

	engineLog.Debug("indexes built", "entity", ent.name, "indexes", len(jobs), "documents", docs, "duration_ms", log.Since(start))

	return more || requeue
}

func (e *Engine) abandonPass(ent *entity, pass *buildPass, jobs []claimedBuild) {
	ent.apply.Lock()
	defer ent.apply.Unlock()

	delete(ent.passes, pass)
	for _, j := range jobs {
		if st := ent.building[j.name]; st == j.st {
			st.pass = nil
		}
	}
}

func (e *Engine) commitPass(ent *entity, snap *root, pass *buildPass, jobs []claimedBuild, trees []pagetree.Tree) bool {
	ent.apply.Lock()
	defer ent.apply.Unlock()

	delete(ent.passes, pass)

	cur := ent.root.Load()
	after := map[uint32]map[string]any{}
	current := func(seq uint32) map[string]any {
		doc, ok := after[seq]
		if !ok {
			if s, found := cur.slotBySeq(seq, false); found {
				doc = e.decodeSlot(s)
			}
			after[seq] = doc
		}
		return doc
	}

	tok := pagetree.NewToken()
	defer tok.Freeze()

	var nr *root
	requeue := false
	for i, j := range jobs {
		st := ent.building[j.name]
		idx, still := cur.indexes[j.name]
		if st != j.st || !still || idx.ready || idx.dormant {
			continue
		}

		if cur.gen != snap.gen {
			st.pass = nil
			requeue = true
			continue
		}

		tree := trees[i].WithToken(tok)
		for seq, before := range pass.before {
			tree, _ = applyDocChange(j.spec, tree, seq, before, current(seq))
		}

		delete(ent.building, j.name)
		if nr == nil {
			c := *cur
			c.indexes = cloneIndexes(cur.indexes)
			nr = &c
		}
		nr.indexes[j.name] = &index{spec: j.spec, ready: true, tree: tree.WithToken(nil)}
	}

	if nr != nil {
		ent.root.Store(nr)
	}

	return requeue
}

func (e *Engine) DropIdleIndexes(idle time.Duration) int {
	e.catalogMu.RLock()
	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}
	e.catalogMu.RUnlock()

	cutoff := time.Now().Add(-idle)
	dropped := 0
	for _, ent := range ents {
		ent.apply.Lock()
		r := ent.root.Load()
		var nr *root
		for name, idx := range r.indexes {
			if !idx.ready || idx.dormant {
				continue
			}

			last := ent.lastUse(name)
			if last.IsZero() {
				ent.touch(name)
				continue
			}

			if !last.Before(cutoff) {
				continue
			}

			if nr == nil {
				c := *r
				c.indexes = cloneIndexes(r.indexes)
				nr = &c
			}
			nr.indexes[name] = &index{spec: idx.spec, dormant: true}
			dropped++
		}

		if nr != nil {
			ent.root.Store(nr)
		}
		ent.apply.Unlock()
	}

	if dropped > 0 {
		engineLog.Debug("idle indexes released", "indexes", dropped, "idle_minutes", int(idle.Minutes()))
	}

	return dropped
}

type IndexState struct {
	Entity   string
	Index    string
	State    string
	Entries  int
	LastUsed time.Time
}

func (e *Engine) IndexStates() []IndexState {
	e.catalogMu.RLock()
	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}
	e.catalogMu.RUnlock()

	out := []IndexState{}
	for _, ent := range ents {
		for name, idx := range ent.root.Load().indexes {
			st := IndexState{Entity: ent.name, Index: name, LastUsed: ent.lastUse(name)}
			if idx.spec.Kind == IndexSearch {
				st.Index = "search"
			}
			switch {
			case idx.dormant:
				st.State = "released"
			case idx.ready:
				st.State = "ready"
				st.Entries = idx.tree.Len()
			default:
				st.State = "building"
			}
			out = append(out, st)
		}

	}

	sort.Slice(out, func(i, j int) bool {
		if out[i].Entity != out[j].Entity {
			return out[i].Entity < out[j].Entity
		}
		return out[i].Index < out[j].Index
	})

	return out
}
