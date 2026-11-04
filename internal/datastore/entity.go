package datastore

import (
	"encoding/json"
	"maps"
	"sort"
	"sync"
	"sync/atomic"

	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/ptree"
)

type slot struct {
	seq  uint32
	id   string
	lsn  uint64
	loc  journal.Location
	data []byte
}

type index struct {
	spec    IndexSpec
	ready   bool
	dormant bool
	tree    ptree.Tree[struct{}]
}

type root struct {
	gen     uint64
	nextSeq uint32
	slots   ptree.Tree[*slot]
	ids     ptree.Tree[*slot]
	indexes map[string]*index
	version uint64
	refs    []RefPath
}

type buildState struct {
	order uint64
	pass  *buildPass
}

type buildPass struct {
	before map[uint32]map[string]any
}

type entity struct {
	name     string
	apply    sync.Mutex
	root     atomic.Pointer[root]
	building map[string]*buildState
	passes   map[*buildPass]struct{}
	search   atomic.Pointer[trigramIndex]
	queued   atomic.Bool
	usage    sync.Map
}

func newEntity(name string) *entity {
	e := &entity{name: name, building: map[string]*buildState{}, passes: map[*buildPass]struct{}{}}
	e.root.Store(&root{indexes: map[string]*index{}})

	return e
}

type draft struct {
	e        *Engine
	ent      *entity
	r        root
	indexes  map[string]*index
	cloned   bool
	refsSeen map[RefPath]bool
	released []ptree.Tree[*slot]
}

func (e *Engine) newDraft(ent *entity) *draft {
	r := *ent.root.Load()

	return &draft{e: e, ent: ent, r: r, indexes: r.indexes}
}

func (d *draft) mutableIndexes() map[string]*index {
	if !d.cloned {
		d.indexes = maps.Clone(d.indexes)
		d.cloned = true
	}

	return d.indexes
}

func (d *draft) publish() {
	d.r.indexes = d.indexes
	if len(d.refsSeen) > 0 {
		refs := append([]RefPath(nil), d.r.refs...)
		for ref := range d.refsSeen {
			refs = append(refs, ref)
		}
		sort.Slice(refs, func(i, j int) bool {
			if refs[i].Path != refs[j].Path {
				return refs[i].Path < refs[j].Path
			}
			return refs[i].Target < refs[j].Target
		})
		d.r.refs = refs
	}

	r := d.r
	d.ent.root.Store(&r)
}

func (d *draft) hasRef(ref RefPath) bool {
	if d.refsSeen[ref] {
		return true
	}

	for _, r := range d.r.refs {
		if r == ref {
			return true
		}
	}

	return false
}

func (d *draft) needsDecode() bool {
	return len(d.indexes) > 0 || len(d.ent.building) > 0 || len(d.ent.passes) > 0 || d.ent.search.Load() != nil
}

func (d *draft) put(rec *journal.Record, doc map[string]any, replay bool) {
	if rec.Gen != d.r.gen {
		return
	}

	old, exists := d.r.ids.Get(rec.Key)
	if exists && replay && rec.LSN <= old.lsn {
		return
	}

	s := &slot{id: rec.Key, lsn: rec.LSN, loc: rec.Loc}
	if d.e.inline {
		s.data = append([]byte(nil), rec.Value...)
	} else {
		d.e.cache.put(rec.LSN, append([]byte(nil), rec.Value...))
	}

	if exists {
		s.seq = old.seq
		if rec.Kind == journal.KindDocMove && old.data != nil {
			s.data = old.data
		}
	} else {
		if rec.Kind == journal.KindDocMove {
			return
		}

		s.seq = d.r.nextSeq
		d.r.nextSeq++
	}

	d.r.slots, _ = d.r.slots.Put(seqKey(s.seq), s)
	d.r.ids, _ = d.r.ids.Put(s.id, s)

	if rec.Kind == journal.KindDocMove {
		return
	}

	d.r.version = rec.LSN

	if doc == nil {
		doc = decodeDoc(rec.Value)
	}

	for _, ref := range DocumentRefs(doc) {
		if !d.hasRef(ref) {
			if d.refsSeen == nil {
				d.refsSeen = map[RefPath]bool{}
			}
			d.refsSeen[ref] = true
		}
	}

	if !d.needsDecode() {
		return
	}

	var oldDoc map[string]any
	if exists {
		oldDoc = d.e.decodeSlot(old)
	}

	d.updateIndexes(s.seq, oldDoc, doc)
}

func (d *draft) del(rec *journal.Record, replay bool) {
	if rec.Gen != d.r.gen {
		return
	}

	old, exists := d.r.ids.Get(rec.Key)
	if !exists || (replay && old.lsn >= rec.LSN) {
		return
	}

	var oldDoc map[string]any
	if d.needsDecode() {
		oldDoc = d.e.decodeSlot(old)
	}

	d.r.slots, _ = d.r.slots.Delete(seqKey(old.seq))
	d.r.ids, _ = d.r.ids.Delete(old.id)
	d.r.version = rec.LSN
	if !d.e.inline {
		d.e.cache.drop(old.lsn)
	}

	if oldDoc != nil {
		d.updateIndexes(old.seq, oldDoc, nil)
	}
}

func (d *draft) reset(rec *journal.Record) {
	if rec.Gen <= d.r.gen {
		return
	}

	indexes := map[string]*index{}
	for name, idx := range d.indexes {
		indexes[name] = &index{spec: idx.spec, ready: idx.ready, dormant: idx.dormant}
	}

	if d.r.slots.Len() > 0 {
		d.released = append(d.released, d.r.slots)
	}

	d.r = root{gen: rec.Gen, version: rec.LSN}
	d.indexes = indexes
	d.cloned = true
	d.refsSeen = nil

	if t := d.ent.search.Load(); t != nil {
		d.ent.search.Store(nil)
	}
}

func (d *draft) updateIndexes(seq uint32, oldDoc, newDoc map[string]any) {
	for pass := range d.ent.passes {
		if _, seen := pass.before[seq]; !seen {
			pass.before[seq] = oldDoc
		}
	}

	if t := d.ent.search.Load(); t != nil && newDoc != nil {
		t.add(seq, newDoc)
	}

	if len(d.indexes) == 0 {
		return
	}

	var changed map[string]*index
	for name, idx := range d.indexes {
		if !idx.ready {
			continue
		}

		oldKeys := uniqueKeys(extractKeys(idx.spec, oldDoc))
		newKeys := uniqueKeys(extractKeys(idx.spec, newDoc))
		if equalStrings(oldKeys, newKeys) {
			continue
		}

		tree := idx.tree
		removed, added := diffSorted(oldKeys, newKeys)
		for _, k := range removed {
			tree, _ = tree.Delete(k + seqKey(seq))
		}

		for _, k := range added {
			tree, _ = tree.Put(k+seqKey(seq), struct{}{})
		}

		if changed == nil {
			changed = map[string]*index{}
		}
		changed[name] = &index{spec: idx.spec, ready: true, tree: tree}
	}

	if len(changed) == 0 {
		return
	}

	m := d.mutableIndexes()
	for name, idx := range changed {
		m[name] = idx
	}
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}

	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}

	return true
}

func diffSorted(old, new []string) ([]string, []string) {
	var removed, added []string
	i, j := 0, 0
	for i < len(old) || j < len(new) {
		switch {
		case j >= len(new) || (i < len(old) && old[i] < new[j]):
			removed = append(removed, old[i])
			i++
		case i >= len(old) || new[j] < old[i]:
			added = append(added, new[j])
			j++
		default:
			i++
			j++
		}
	}

	return removed, added
}

func DecodeMap(b []byte) (map[string]any, error) {
	var doc map[string]any
	if err := json.Unmarshal(b, &doc); err != nil {
		return nil, err
	}

	return doc, nil
}

func decodeDoc(b []byte) map[string]any {
	var doc map[string]any
	if err := json.Unmarshal(b, &doc); err != nil {
		return nil
	}

	return doc
}

func (e *Engine) slotBytes(s *slot) ([]byte, error) {
	if s.data != nil {
		return s.data, nil
	}

	if b, ok := e.cache.get(s.lsn); ok {
		return b, nil
	}

	b, err := e.j.ReadValue(s.loc)
	if err != nil {
		return nil, err
	}

	e.cache.put(s.lsn, b)

	return b, nil
}

func (e *Engine) decodeSlot(s *slot) map[string]any {
	b, err := e.slotBytes(s)
	if err != nil {
		return nil
	}

	return decodeDoc(b)
}
