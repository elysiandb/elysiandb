package datastore

import (
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/pagetree"
)

type IndexOrigin string

const (
	OriginQuery   IndexOrigin = "query"
	OriginHook    IndexOrigin = "hook"
	OriginAdvisor IndexOrigin = "advisor"
)

type indexMeta struct {
	mu      sync.Mutex
	origin  IndexOrigin
	created time.Time
	uses    atomic.Int64
	bytes   atomic.Int64
}

func (ent *entity) meta(name string) *indexMeta {
	if v, ok := ent.metas.Load(name); ok {
		return v.(*indexMeta)
	}

	v, _ := ent.metas.LoadOrStore(name, &indexMeta{origin: OriginQuery, created: time.Now()})

	return v.(*indexMeta)
}

func (m *indexMeta) Origin() IndexOrigin {
	m.mu.Lock()
	defer m.mu.Unlock()

	return m.origin
}

func (m *indexMeta) setOrigin(o IndexOrigin) {
	m.mu.Lock()
	m.origin = o
	m.mu.Unlock()
}

func pinKey(spec IndexSpec) string {
	switch spec.Kind {
	case IndexFilter, IndexBitmap:
		return "filter:" + spec.Field
	}

	return spec.Name()
}

func (ent *entity) pinned(spec IndexSpec) bool {
	v, ok := ent.pins.Load(pinKey(spec))
	return ok && v.(*atomic.Int64).Load() > 0
}

func (e *Engine) PinIndex(name string, spec IndexSpec) {
	ent := e.entity(name, true)
	v, _ := ent.pins.LoadOrStore(pinKey(spec), new(atomic.Int64))
	v.(*atomic.Int64).Add(1)
}

func (e *Engine) UnpinIndex(name string, spec IndexSpec) {
	ent := e.entity(name, false)
	if ent == nil {
		return
	}

	if v, ok := ent.pins.Load(pinKey(spec)); ok && v.(*atomic.Int64).Add(-1) <= 0 {
		ent.pins.Delete(pinKey(spec))
	}
}

func (e *Engine) EnsureIndexWith(name string, spec IndexSpec, origin IndexOrigin) bool {
	ent := e.entity(name, true)
	if _, ok := ent.root.Load().indexes[spec.Name()]; !ok {
		ent.meta(spec.Name()).setOrigin(origin)
	}

	return e.EnsureIndex(name, spec)
}

func indexKindName(k IndexKind) string {
	switch k {
	case IndexFilter:
		return "ordered"
	case IndexSort:
		return "sort"
	case IndexRefs:
		return "references"
	case IndexSearch:
		return "search"
	case IndexBitmap:
		return "bitmap"
	case IndexComposite:
		return "composite"
	}

	return string(k)
}

type IndexState struct {
	Entity   string
	Index    string
	Kind     string
	Origin   string
	State    string
	Pinned   bool
	Entries  int
	Bytes    int64
	Uses     int64
	LastUsed time.Time
}

func (e *Engine) IndexStates() []IndexState {
	out := []IndexState{}
	for _, ent := range e.allEntities() {
		for name, idx := range ent.root.Load().indexes {
			m := ent.meta(name)
			st := IndexState{
				Entity:   ent.name,
				Index:    name,
				Kind:     indexKindName(idx.spec.Kind),
				Origin:   string(m.Origin()),
				Pinned:   ent.pinned(idx.spec),
				Uses:     m.uses.Load(),
				Bytes:    m.bytes.Load(),
				LastUsed: ent.lastUse(name),
			}
			if idx.spec.Kind == IndexSearch {
				st.Index = "search"
			}
			switch {
			case idx.dormant:
				st.State = "released"
				st.Bytes, st.Entries = 0, 0
			case idx.ready:
				st.State = "ready"
				st.Entries = (&IndexView{idx: idx}).Len()
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

func (e *Engine) measureIndexes() {
	for _, ent := range e.allEntities() {
		r := ent.root.Load()
		for name, idx := range r.indexes {
			if !idx.ready {
				continue
			}

			var size int64
			idx.tree.WalkStored(func(_ uint64, n uint32) {
				size += int64(n) + pageHeaderSize
			})
			_ = idx.tree.ForEachDirty(func(_ *pagetree.Ref, p *pagetree.Page) error {
				size += int64(p.Bytes())
				return nil
			})

			ent.meta(name).bytes.Store(size)
		}
	}
}
