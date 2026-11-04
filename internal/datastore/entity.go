package datastore

import (
	"encoding/binary"
	"encoding/json"
	"maps"
	"sort"
	"sync"
	"sync/atomic"
	"unsafe"

	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/pagetree"
)

type slot struct {
	seq uint32
	id  string
	lsn uint64
	loc journal.Location
}

func encodeSlot(dst []byte, s slot) []byte {
	dst = binary.AppendUvarint(dst, s.lsn)
	dst = binary.AppendUvarint(dst, uint64(s.loc.Segment))
	dst = binary.AppendUvarint(dst, uint64(s.loc.Offset))
	dst = binary.AppendUvarint(dst, uint64(s.loc.Length))

	return append(dst, s.id...)
}

func decodeSlotValue(seq uint32, b []byte, shared bool) slot {
	s := slot{seq: seq}
	pos := 0
	next := func() uint64 {
		v, n := binary.Uvarint(b[pos:])
		if n <= 0 {
			return 0
		}
		pos += n
		return v
	}

	s.lsn = next()
	s.loc.Segment = uint32(next())
	s.loc.Offset = uint32(next())
	s.loc.Length = uint32(next())

	idBytes := b[pos:]
	switch {
	case len(idBytes) == 0:
	case shared:
		s.id = unsafe.String(&idBytes[0], len(idBytes))
	default:
		s.id = string(idBytes)
	}

	return s
}

func seqBytes(seq uint32) []byte {
	var b [4]byte
	binary.BigEndian.PutUint32(b[:], seq)

	return b[:]
}

func seqOf(b []byte) uint32 {
	return binary.BigEndian.Uint32(b)
}

type index struct {
	spec    IndexSpec
	ready   bool
	dormant bool
	tree    pagetree.Tree
}

type root struct {
	gen     uint64
	nextSeq uint32
	slots   pagetree.Tree
	ids     pagetree.Tree
	indexes map[string]*index
	version uint64
	refs    []RefPath
}

func (r *root) slotBySeq(seq uint32, shared bool) (slot, bool) {
	b, ok := r.slots.Get(seqKey(seq))
	if !ok {
		return slot{}, false
	}

	return decodeSlotValue(seq, b, shared), true
}

func (r *root) slotByID(id string, shared bool) (slot, bool) {
	b, ok := r.ids.Get(id)
	if !ok {
		return slot{}, false
	}

	return r.slotBySeq(seqOf(b), shared)
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
	queued   atomic.Bool
	usage    sync.Map
	metas    sync.Map
	pins     sync.Map
	stale    atomic.Int64
}

func (e *Engine) newEntity(name string) *entity {
	ent := &entity{name: name, building: map[string]*buildState{}, passes: map[*buildPass]struct{}{}}
	ent.root.Store(e.emptyRoot(0, 0))

	return ent
}

func (e *Engine) emptyRoot(gen, version uint64) *root {
	return &root{gen: gen, version: version, slots: pagetree.New(e.pages), ids: pagetree.New(e.pages), indexes: map[string]*index{}}
}

type draft struct {
	e        *Engine
	ent      *entity
	r        root
	indexes  map[string]*index
	cloned   bool
	refsSeen map[RefPath]bool
	released []pagetree.Tree
	tok      *pagetree.Token
}

func (e *Engine) newDraft(ent *entity) *draft {
	r := *ent.root.Load()
	tok := pagetree.NewToken()
	r.slots = r.slots.WithToken(tok)
	r.ids = r.ids.WithToken(tok)

	return &draft{e: e, ent: ent, r: r, indexes: r.indexes, tok: tok}
}

func (d *draft) mutableIndexes() map[string]*index {
	if !d.cloned {
		d.indexes = maps.Clone(d.indexes)
		d.cloned = true
	}

	return d.indexes
}

func (d *draft) publish() {
	d.tok.Freeze()
	d.r.slots = d.r.slots.WithToken(nil)
	d.r.ids = d.r.ids.WithToken(nil)
	if d.cloned {
		for name, idx := range d.indexes {
			if idx.tree.Root() != nil {
				d.indexes[name] = &index{spec: idx.spec, ready: idx.ready, dormant: idx.dormant, tree: idx.tree.WithToken(nil)}
			}
		}
	}

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
	return len(d.indexes) > 0 || len(d.ent.building) > 0 || len(d.ent.passes) > 0
}

func (d *draft) put(rec *journal.Record, doc map[string]any, replay bool) {
	if rec.Gen != d.r.gen {
		return
	}

	old, exists := d.r.slotByID(rec.Key, false)
	if exists && replay && rec.LSN <= old.lsn {
		return
	}

	if !exists && rec.Kind == journal.KindDocMove {
		return
	}

	s := slot{id: rec.Key, lsn: rec.LSN, loc: rec.Loc}
	d.e.cache.put(rec.LSN, append([]byte(nil), rec.Value...))
	d.e.live.add(rec.Loc.Segment, int64(rec.Loc.Length))

	if exists {
		s.seq = old.seq
		d.e.cache.drop(old.lsn)
		d.e.live.add(old.loc.Segment, -int64(old.loc.Length))
	} else {
		s.seq = d.r.nextSeq
		d.r.nextSeq++
		d.r.ids, _ = d.r.ids.Put(s.id, seqBytes(s.seq))
	}

	d.r.slots, _ = d.r.slots.Put(seqKey(s.seq), encodeSlot(nil, s))

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

	old, exists := d.r.slotByID(rec.Key, false)
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
	d.e.cache.drop(old.lsn)
	d.e.live.add(old.loc.Segment, -int64(old.loc.Length))

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
		next := &index{spec: idx.spec, ready: idx.ready, dormant: idx.dormant}
		if idx.ready {
			next.tree = pagetree.New(d.e.pages)
		}
		indexes[name] = next
	}

	if d.r.slots.Len() > 0 {
		d.released = append(d.released, d.r.slots)
	}

	fresh := d.e.emptyRoot(rec.Gen, rec.LSN)
	d.r = *fresh
	d.r.slots = d.r.slots.WithToken(d.tok)
	d.r.ids = d.r.ids.WithToken(d.tok)
	d.indexes = indexes
	d.cloned = true
	d.refsSeen = nil
}

func (d *draft) updateIndexes(seq uint32, oldDoc, newDoc map[string]any) {
	for pass := range d.ent.passes {
		if _, seen := pass.before[seq]; !seen {
			pass.before[seq] = oldDoc
		}
	}

	if len(d.indexes) == 0 {
		return
	}

	var changed map[string]*index
	for name, idx := range d.indexes {
		if !idx.ready {
			continue
		}

		if idx.spec.Kind == IndexSearch && oldDoc != nil && d.ent.stale.Add(1) > 2*int64(d.r.slots.Len())+staleSearchSlack {
			d.ent.stale.Store(0)
			if changed == nil {
				changed = map[string]*index{}
			}
			changed[name] = &index{spec: idx.spec, dormant: true}
			continue
		}

		tree, ok := applyDocChange(idx.spec, idx.tree.WithToken(d.tok), seq, oldDoc, newDoc)
		if !ok {
			continue
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

func applyDocChange(spec IndexSpec, tree pagetree.Tree, seq uint32, oldDoc, newDoc map[string]any) (pagetree.Tree, bool) {
	if spec.Kind == IndexSearch {
		_, added := diffGrams(searchGrams(oldDoc), searchGrams(newDoc))
		if len(added) == 0 {
			return tree, false
		}

		return updatePostings(tree, seq, nil, added), true
	}

	if spec.Kind == IndexComposite {
		return applyCompositeChange(spec, tree, seq, oldDoc, newDoc)
	}

	oldKeys := uniqueKeys(extractKeys(spec, oldDoc))
	newKeys := uniqueKeys(extractKeys(spec, newDoc))
	if equalStrings(oldKeys, newKeys) {
		return tree, false
	}

	removed, added := diffSorted(oldKeys, newKeys)
	if spec.Kind == IndexBitmap {
		return updateBitmap(tree, seq, removed, added), true
	}

	sk := seqKey(seq)
	for _, k := range removed {
		tree, _ = tree.Delete(k + sk)
	}

	for _, k := range added {
		tree, _ = tree.Put(k+sk, nil)
	}

	return tree, true
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

func (e *Engine) slotBytes(s slot) ([]byte, error) {
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

func (e *Engine) decodeSlot(s slot) map[string]any {
	b, err := e.slotBytes(s)
	if err != nil {
		return nil
	}

	return decodeDoc(b)
}

type liveBytes struct {
	m sync.Map
}

func (l *liveBytes) add(seg uint32, n int64) {
	if n == 0 {
		return
	}

	v, ok := l.m.Load(seg)
	if !ok {
		v, _ = l.m.LoadOrStore(seg, new(atomic.Int64))
	}
	v.(*atomic.Int64).Add(n)
}

func (l *liveBytes) snapshot() map[uint32]int64 {
	out := map[uint32]int64{}
	l.m.Range(func(k, v any) bool {
		if n := v.(*atomic.Int64).Load(); n > 0 {
			out[k.(uint32)] = n
		}
		return true
	})

	return out
}

func (l *liveBytes) reset(values map[uint32]int64) {
	l.m.Clear()
	for seg, n := range values {
		l.add(seg, n)
	}
}
