package datastore

import (
	"sort"
	"time"

	"github.com/taymour/elysiandb/internal/memory"
)

const (
	advisorMinDocs      = 4096
	bitmapMaxDistinct   = 64
	orderedMinDistinct  = 1024
	pressureReleaseIdle = 10 * time.Minute
	pressureSweeps      = 3
)

var distinctKinds = []byte{KeyRaw, KeyNumber, KeyBool, KeyElement}

func (v *IndexView) distinct(limit int) int {
	if v.bitmap() {
		n := 0
		for _, kind := range distinctKinds {
			last := ""
			v.idx.tree.AscendFrom(string([]byte{kind}), func(k string, _ []byte) bool {
				if k[0] != kind {
					return false
				}
				value := bitmapValue(k)
				if value != last {
					last = value
					n++
				}
				return n <= limit
			})
			if n > limit {
				return n
			}
		}
		return n
	}

	n := 0
	for _, kind := range distinctKinds {
		from := string([]byte{kind})
		for n <= limit {
			key, ok := "", false
			v.idx.tree.AscendFrom(from, func(k string, _ []byte) bool {
				key, ok = k, k[0] == kind
				return false
			})
			if !ok {
				break
			}
			n++
			from = KeyPayload(key) + "\xff\xff\xff\xff\xff"
		}
		if n > limit {
			return n
		}
	}

	return n
}

func (e *Engine) Advise() {
	e.measureIndexes()

	for _, ent := range e.allEntities() {
		r := ent.root.Load()
		if r.slots.Len() < advisorMinDocs {
			continue
		}

		fields := map[string]bool{}
		for _, idx := range r.indexes {
			if idx.spec.Kind == IndexFilter || idx.spec.Kind == IndexBitmap {
				fields[idx.spec.Field] = true
			}
		}

		for field := range fields {
			e.adviseFilter(ent, r, field)
		}
	}

	e.releaseUnderPressure()
}

func (e *Engine) adviseFilter(ent *entity, r *root, field string) {
	ordered := IndexSpec{Kind: IndexFilter, Field: field}
	bitmap := IndexSpec{Kind: IndexBitmap, Field: field}
	o, hasOrdered := r.indexes[ordered.Name()]
	b, hasBitmap := r.indexes[bitmap.Name()]
	orderedReady := hasOrdered && o.ready && !o.dormant
	bitmapReady := hasBitmap && b.ready && !b.dormant

	switch {
	case orderedReady && bitmapReady:
		if (&IndexView{idx: b}).distinct(orderedMinDistinct) > orderedMinDistinct {
			e.dropIndex(ent, bitmap.Name())
		} else {
			e.dropIndex(ent, ordered.Name())
		}
	case orderedReady && !hasBitmap:
		if (&IndexView{idx: o}).distinct(bitmapMaxDistinct) <= bitmapMaxDistinct {
			e.EnsureIndexWith(ent.name, bitmap, ent.meta(ordered.Name()).Origin())
		}
	case bitmapReady && !hasOrdered:
		if (&IndexView{idx: b}).distinct(orderedMinDistinct) > orderedMinDistinct {
			e.EnsureIndexWith(ent.name, ordered, ent.meta(bitmap.Name()).Origin())
		}
	}
}

func (e *Engine) dropIndex(ent *entity, name string) {
	ent.apply.Lock()
	defer ent.apply.Unlock()

	r := ent.root.Load()
	if _, ok := r.indexes[name]; !ok {
		return
	}

	nr := *r
	nr.indexes = cloneIndexes(r.indexes)
	delete(nr.indexes, name)
	delete(ent.building, name)
	ent.root.Store(&nr)
	ent.metas.Delete(name)
	ent.usage.Delete(name)
}

func (e *Engine) releaseUnderPressure() {
	if memory.Current() < memory.High {
		e.pressureSweeps = 0
		return
	}

	e.pressureSweeps++
	if e.pressureSweeps < pressureSweeps {
		return
	}

	type candidate struct {
		ent  *entity
		name string
		uses int64
	}

	cutoff := time.Now().Add(-pressureReleaseIdle)
	var list []candidate
	for _, ent := range e.allEntities() {
		for name, idx := range ent.root.Load().indexes {
			if !idx.ready || idx.dormant || ent.pinned(idx.spec) || idx.spec.Kind == IndexRefs {
				continue
			}
			if last := ent.lastUse(name); !last.IsZero() && last.Before(cutoff) {
				list = append(list, candidate{ent: ent, name: name, uses: ent.meta(name).uses.Load()})
			}
		}
	}

	sort.Slice(list, func(i, j int) bool { return list[i].uses < list[j].uses })
	for i := 0; i < len(list) && i < 4; i++ {
		e.releaseIndex(list[i].ent, list[i].name)
	}
}

func (e *Engine) releaseIndex(ent *entity, name string) {
	ent.apply.Lock()
	defer ent.apply.Unlock()

	r := ent.root.Load()
	idx, ok := r.indexes[name]
	if !ok || !idx.ready {
		return
	}

	nr := *r
	nr.indexes = cloneIndexes(r.indexes)
	nr.indexes[name] = &index{spec: idx.spec, dormant: true}
	ent.root.Store(&nr)
}
