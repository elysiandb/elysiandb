package datastore

import (
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/ptree"
)

const (
	smallEntityDocs   = 512
	maxBatchEntries   = 2_000_000
	maxBatchIndexes   = 32
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

func (e *Engine) claim(ent *entity) (*root, *buildPass, []claimedBuild, *trigramIndex, bool) {
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
	more := len(pending) > limit
	if more {
		pending = pending[:limit]
	}

	var search *trigramIndex
	if t := ent.search.Load(); t != nil && !t.claimed {
		t.claimed = true
		search = t
	}

	if len(pending) == 0 && search == nil {
		return nil, nil, nil, nil, false
	}

	pass := &buildPass{before: map[uint32]map[string]any{}}
	for _, c := range pending {
		c.st.pass = pass
	}
	ent.passes[pass] = struct{}{}

	return snap, pass, pending, search, more
}

func (e *Engine) buildPass(ent *entity) bool {
	snap, pass, jobs, search, more := e.claim(ent)
	if pass == nil {
		return false
	}

	start := time.Now()
	keys := make([][]string, len(jobs))
	docs := 0
	snap.slots.AscendAt(0, func(_ string, s *slot) bool {
		doc := e.decodeSlot(s)
		if search != nil && doc != nil {
			search.add(s.seq, doc)
		}

		for i, j := range jobs {
			for _, k := range uniqueKeys(extractKeys(j.spec, doc)) {
				keys[i] = append(keys[i], k+seqKey(s.seq))
			}
		}

		docs++
		return true
	})

	if search != nil {
		search.mu.Lock()
		search.ready = true
		search.docs = docs
		search.updates = docs
		search.mu.Unlock()
	}

	trees := make([]ptree.Tree[struct{}], len(jobs))
	for i := range jobs {
		sort.Strings(keys[i])
		trees[i] = ptree.FromSorted(keys[i], make([]struct{}, len(keys[i])))
		keys[i] = nil
	}

	requeue := e.commitPass(ent, snap, pass, jobs, trees)

	if len(jobs) > 0 {
		engineLog.Debug("indexes built", "entity", ent.name, "indexes", len(jobs), "search", search != nil, "documents", docs, "duration_ms", log.Since(start))
	}

	return more || requeue
}

func (e *Engine) commitPass(ent *entity, snap *root, pass *buildPass, jobs []claimedBuild, trees []ptree.Tree[struct{}]) bool {
	ent.apply.Lock()
	defer ent.apply.Unlock()

	delete(ent.passes, pass)

	cur := ent.root.Load()
	after := map[uint32]map[string]any{}
	current := func(seq uint32) map[string]any {
		doc, ok := after[seq]
		if !ok {
			if s, found := cur.slots.Get(seqKey(seq)); found {
				doc = e.decodeSlot(s)
			}
			after[seq] = doc
		}
		return doc
	}

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

		tree := trees[i]
		for seq, before := range pass.before {
			for _, k := range uniqueKeys(extractKeys(j.spec, before)) {
				tree, _ = tree.Delete(k + seqKey(seq))
			}

			for _, k := range uniqueKeys(extractKeys(j.spec, current(seq))) {
				tree, _ = tree.Put(k+seqKey(seq), struct{}{})
			}
		}

		delete(ent.building, j.name)
		if nr == nil {
			c := *cur
			c.indexes = cloneIndexes(cur.indexes)
			nr = &c
		}
		nr.indexes[j.name] = &index{spec: j.spec, ready: true, tree: tree}
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

		if t := ent.search.Load(); t != nil {
			st := IndexState{Entity: ent.name, Index: "search", State: "building"}
			t.mu.RLock()
			if t.ready {
				st.State = "ready"
				st.Entries = len(t.postings)
			}
			t.mu.RUnlock()
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
