package datastore

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"sort"
	"sync/atomic"
	"time"

	"github.com/cockroachdb/pebble/v2"
)

const maxCommitAttempts = 64

type opKind uint8

const (
	opPut opKind = iota + 1
	opDelete
	opReset
	opCreateType
	opDropType
	opClearAll
)

type txOp struct {
	kind   opKind
	entity string
	id     string
	value  []byte
	doc    map[string]any
}

type readEntry struct {
	value []byte
	lsn   uint64
	seq   uint32
	found bool
}

type Tx struct {
	e       *Engine
	reads   map[string]uint64
	cache   map[string]readEntry
	ops     []txOp
	overlay map[string]int
	resets  map[string]bool
	clear   bool
	decoded map[string]decodedDoc
}

type decodedDoc struct {
	value []byte
	doc   map[string]any
}

func docRef(entity, id string) string {
	return entity + "\x00" + id
}

func splitDocRef(key string) (string, string) {
	for i := 0; i < len(key); i++ {
		if key[i] == 0 {
			return key[:i], key[i+1:]
		}
	}

	return key, ""
}

func (e *Engine) newTx() *Tx {
	return &Tx{e: e, reads: map[string]uint64{}, cache: map[string]readEntry{}, overlay: map[string]int{}, resets: map[string]bool{}}
}

type gate struct {
	inside atomic.Int64
	shut   atomic.Bool
}

func (g *gate) enter() bool {
	g.inside.Add(1)
	if g.shut.Load() {
		g.inside.Add(-1)
		return false
	}

	return true
}

func (g *gate) leave() {
	g.inside.Add(-1)
}

func (g *gate) shutAndWait() {
	g.shut.Store(true)
	for g.inside.Load() > 0 {
		time.Sleep(time.Millisecond)
	}
}

func (e *Engine) enter() bool {
	return !e.closed.Load() && e.writers.enter()
}

func (e *Engine) leave() {
	e.writers.leave()
}

func (e *Engine) Update(fn func(tx *Tx) error) error {
	if !e.enter() {
		return ErrClosed
	}
	defer e.leave()

	for attempt := 0; ; attempt++ {
		tx := e.newTx()
		if err := fn(tx); err != nil {
			return err
		}

		a, err := e.commit(tx)
		if errors.Is(err, ErrConflict) && attempt < maxCommitAttempts {
			e.stats.conflicts.Add(1)
			if attempt > 4 {
				time.Sleep(time.Duration(attempt) * 50 * time.Microsecond)
			}
			continue
		}

		if err != nil {
			return err
		}

		return a.finish()
	}
}

func (tx *Tx) lookup(entity, id string) (*txOp, bool) {
	if i, ok := tx.overlay[docRef(entity, id)]; ok {
		return &tx.ops[i], true
	}

	if tx.resets[entity] || tx.clear {
		return nil, true
	}

	return nil, false
}

func (e *Engine) readLatest(entity, id string) (readEntry, error) {
	ent := e.entity(entity, false)
	if ent == nil {
		return readEntry{}, nil
	}

	rec := ent.rec.Load()
	if rec == nil {
		return readEntry{}, nil
	}

	return e.readDoc(rec.id, id)
}

func (e *Engine) readDoc(entityID uint32, id string) (readEntry, error) {
	sb, ok, err := e.get(idKey(entityID, id))
	if err != nil || !ok || len(sb) != 4 {
		return readEntry{}, err
	}

	seq := binary.BigEndian.Uint32(sb)
	b, ok, err := e.get(docKey(entityID, seq))
	if err != nil || !ok {
		return readEntry{}, err
	}

	lsn, got, value, valid := decodeDocValue(b)
	if !valid || string(got) != id {
		return readEntry{}, nil
	}

	return readEntry{value: value, lsn: lsn, seq: seq, found: true}, nil
}

func (e *Engine) Read(entity, id string) ([]byte, bool) {
	if e.closed.Load() {
		return nil, false
	}

	r, err := e.readLatest(entity, id)
	if err != nil || !r.found {
		return nil, false
	}

	return r.value, true
}

func (tx *Tx) ReadBytes(entity, id string) ([]byte, bool) {
	if op, ok := tx.lookup(entity, id); ok {
		if op == nil || op.kind == opDelete {
			return nil, false
		}

		return op.value, true
	}

	key := docRef(entity, id)
	r, ok := tx.cache[key]
	if !ok {
		var err error
		r, err = tx.e.readLatest(entity, id)
		if err != nil {
			return nil, false
		}
		tx.cache[key] = r
	}

	if _, seen := tx.reads[key]; !seen {
		tx.reads[key] = r.lsn
	}

	if !r.found {
		return nil, false
	}

	return r.value, true
}

func (tx *Tx) ReadShared(entity, id string) map[string]any {
	b, ok := tx.ReadBytes(entity, id)
	if !ok {
		return nil
	}

	key := docRef(entity, id)
	if c, ok := tx.decoded[key]; ok && len(c.value) == len(b) && len(b) > 0 && &c.value[0] == &b[0] {
		return c.doc
	}

	doc := decodeDoc(b)
	if tx.decoded == nil {
		tx.decoded = map[string]decodedDoc{}
	}
	tx.decoded[key] = decodedDoc{value: b, doc: doc}

	return doc
}

func (tx *Tx) Read(entity, id string) map[string]any {
	b, ok := tx.ReadBytes(entity, id)
	if !ok {
		return nil
	}

	return decodeDoc(b)
}

func (tx *Tx) Exists(entity, id string) bool {
	_, ok := tx.ReadBytes(entity, id)
	return ok
}

func (tx *Tx) Put(entity, id string, doc map[string]any) error {
	value, err := json.Marshal(doc)
	if err != nil {
		return err
	}

	return tx.PutBytes(entity, id, value, decodeDoc(value))
}

func (tx *Tx) PutBytes(entity, id string, value []byte, doc map[string]any) error {
	tx.record(txOp{kind: opPut, entity: entity, id: id, value: value, doc: doc})
	return nil
}

func (tx *Tx) Delete(entity, id string) {
	tx.record(txOp{kind: opDelete, entity: entity, id: id})
}

func (tx *Tx) record(op txOp) {
	tx.ops = append(tx.ops, op)
	tx.overlay[docRef(op.entity, op.id)] = len(tx.ops) - 1
}

func (tx *Tx) ResetEntity(entity string) {
	for key, i := range tx.overlay {
		if tx.ops[i].entity == entity {
			delete(tx.overlay, key)
		}
	}

	tx.ops = append(tx.ops, txOp{kind: opReset, entity: entity})
	tx.resets[entity] = true
}

func (tx *Tx) EnsureType(entity string) {
	tx.ops = append(tx.ops, txOp{kind: opCreateType, entity: entity})
}

func (tx *Tx) DropType(entity string) {
	tx.ops = append(tx.ops, txOp{kind: opDropType, entity: entity})
}

func (tx *Tx) ClearAll() {
	tx.ops = append(tx.ops, txOp{kind: opClearAll})
	tx.overlay = map[string]int{}
	tx.clear = true
}

func (tx *Tx) TypeExists(entity string) bool {
	exists := tx.e.TypeExists(entity)
	for _, op := range tx.ops {
		switch {
		case op.kind == opCreateType && op.entity == entity:
			exists = true
		case op.kind == opDropType && op.entity == entity:
			exists = false
		case op.kind == opClearAll:
			exists = false
		}
	}

	return exists
}

func (tx *Tx) Empty() bool {
	return len(tx.ops) == 0
}

type commitCtx struct {
	e      *Engine
	b      *pebble.Batch
	lsn    uint64
	drafts map[string]*draft
	order  []*draft
}

type docState struct {
	exists  bool
	seq     uint32
	lsn     uint64
	value   []byte
	doc     map[string]any
	decoded bool
}

func (st *docState) decode() map[string]any {
	if !st.decoded {
		st.doc = decodeDoc(st.value)
		st.decoded = true
	}

	return st.doc
}

type draft struct {
	c       *commitCtx
	ent     *entity
	base    *entityRecord
	rec     *entityRecord
	reset   bool
	docs    map[string]*docState
	changed bool
	dirty   map[uint32]struct{}
	cleared [][2][]byte
}

func (c *commitCtx) draft(name string) *draft {
	if d := c.drafts[name]; d != nil {
		return d
	}

	ent := c.e.entity(name, true)
	d := &draft{c: c, ent: ent, base: ent.rec.Load(), docs: map[string]*docState{}}
	if d.base != nil {
		d.rec = d.base.clone()
	} else {
		d.rec = &entityRecord{id: c.e.nextID.Add(1) - 1}
	}
	c.drafts[name] = d
	c.order = append(c.order, d)

	return d
}

func (d *draft) lookup(id string) (*docState, error) {
	if st, ok := d.docs[id]; ok {
		return st, nil
	}

	st := &docState{}
	if !d.reset && d.base != nil {
		r, err := d.c.e.readDoc(d.rec.id, id)
		if err != nil {
			return nil, err
		}

		if r.found {
			st = &docState{exists: true, seq: r.seq, lsn: r.lsn, value: r.value}
		}
	}

	d.docs[id] = st

	return st, nil
}

func (d *draft) indexed() bool {
	for _, m := range d.rec.indexes {
		if m.state == stateReady || (m.state == stateBuilding && d.ent.job != nil) {
			return true
		}
	}

	return false
}

func (d *draft) put(id string, value []byte, doc map[string]any) error {
	st, err := d.lookup(id)
	if err != nil {
		return err
	}

	if doc == nil {
		doc = decodeDoc(value)
	}

	var oldDoc map[string]any
	seq := st.seq
	if st.exists {
		if d.indexed() {
			oldDoc = st.decode()
		}
	} else {
		seq = d.rec.nextSeq
		d.rec.nextSeq++
		d.rec.count++
		var sb [4]byte
		binary.BigEndian.PutUint32(sb[:], seq)
		if err := d.c.b.Set(idKey(d.rec.id, id), sb[:], nil); err != nil {
			return err
		}
	}

	if err := d.c.b.Set(docKey(d.rec.id, seq), encodeDocValue(nil, d.c.lsn, id, value), nil); err != nil {
		return err
	}

	for _, ref := range DocumentRefs(doc) {
		if !d.rec.hasRef(ref) {
			d.rec.addRef(ref)
		}
	}

	if err := d.maintain(seq, oldDoc, doc, st.exists); err != nil {
		return err
	}

	d.docs[id] = &docState{exists: true, seq: seq, lsn: d.c.lsn, value: value, doc: doc, decoded: true}
	d.changed = true

	return nil
}

func (d *draft) del(id string) error {
	st, err := d.lookup(id)
	if err != nil || !st.exists {
		return err
	}

	var oldDoc map[string]any
	if d.indexed() {
		oldDoc = st.decode()
	}

	if err := d.c.b.Delete(docKey(d.rec.id, st.seq), nil); err != nil {
		return err
	}

	if err := d.c.b.Delete(idKey(d.rec.id, id), nil); err != nil {
		return err
	}

	d.rec.count--
	if err := d.maintain(st.seq, oldDoc, nil, true); err != nil {
		return err
	}

	d.docs[id] = &docState{}
	d.changed = true

	return nil
}

func (d *draft) resetEntity() error {
	lo, hi := entityRange(d.rec.id)
	if err := d.c.b.DeleteRange(lo, hi, nil); err != nil {
		return err
	}

	if d.rec.count > 0 {
		d.cleared = append(d.cleared, [2][]byte{lo, hi})
	}

	d.rec.gen++
	d.rec.nextSeq = 0
	d.rec.count = 0
	d.rec.refs = nil
	for _, m := range d.rec.indexes {
		m.clear()
		if m.state == stateBuilding {
			m.state = stateReady
		}
	}

	d.reset = true
	d.docs = map[string]*docState{}
	d.dirty = nil
	d.changed = true

	return nil
}

func (d *draft) maintain(seq uint32, oldDoc, newDoc map[string]any, existed bool) error {
	job := d.ent.job
	if d.reset {
		job = nil
	}

	_, pending := d.dirty[seq]
	for _, m := range d.rec.indexes {
		before := oldDoc
		switch m.state {
		case stateReady:
		case stateBuilding:
			if job == nil || !job.covers(m.name) {
				continue
			}
			if !existed || (int64(seq) > job.builtUpTo && !job.dirty.Has(seq) && !pending) {
				before = nil
			}
			if d.dirty == nil {
				d.dirty = map[uint32]struct{}{}
			}
			d.dirty[seq] = struct{}{}
		default:
			continue
		}

		if err := d.c.applyIndex(d.rec.id, m, seq, before, newDoc); err != nil {
			return err
		}
	}

	return nil
}

func (c *commitCtx) applyIndex(entityID uint32, m *indexMeta, seq uint32, oldDoc, newDoc map[string]any) error {
	if m.spec.Kind == IndexSearch {
		oldGrams, newGrams := searchGrams(oldDoc), searchGrams(newDoc)
		removed, added := diffGrams(oldGrams, newGrams)
		chunk := seq >> postingChunkBits
		off := uint16(seq & (postingChunk - 1))
		for _, g := range removed {
			if err := c.b.Merge(indexKey(entityID, m.no, postingEntry(g, chunk)), postingRemoveOp(off), nil); err != nil {
				return err
			}
		}

		for _, g := range added {
			if err := c.b.Merge(indexKey(entityID, m.no, postingEntry(g, chunk)), postingAddOp(off), nil); err != nil {
				return err
			}
		}

		switch {
		case len(oldGrams) == 0 && len(newGrams) > 0:
			m.entries++
		case len(oldGrams) > 0 && len(newGrams) == 0:
			m.entries--
		}

		return nil
	}

	oldKeys := uniqueKeys(extractKeys(m.spec, oldDoc))
	newKeys := uniqueKeys(extractKeys(m.spec, newDoc))
	if equalStrings(oldKeys, newKeys) {
		return nil
	}

	removed, added := diffSorted(oldKeys, newKeys)
	var sk [4]byte
	binary.BigEndian.PutUint32(sk[:], seq)
	counted := m.counted()
	for _, k := range removed {
		if err := c.b.Delete(indexKey(entityID, m.no, append([]byte(k), sk[:]...)), nil); err != nil {
			return err
		}
		m.entries--
		if counted {
			m.addKind(k[0], -1)
		}
	}

	for _, k := range added {
		if err := c.b.Set(indexKey(entityID, m.no, append([]byte(k), sk[:]...)), nil, nil); err != nil {
			return err
		}
		m.entries++
		if counted {
			m.addKind(k[0], 1)
		}
	}

	return nil
}

func (e *Engine) commit(tx *Tx) (applied, error) {
	if len(tx.ops) == 0 {
		return applied{}, nil
	}

	names := map[string]bool{}
	typeOps := tx.clear
	for _, op := range tx.ops {
		switch op.kind {
		case opCreateType, opDropType:
			typeOps = true
		case opClearAll:
		default:
			names[op.entity] = true
		}
	}

	if tx.clear {
		for _, ent := range e.allEntities() {
			names[ent.name] = true
		}
	}

	sorted := make([]string, 0, len(names))
	for name := range names {
		sorted = append(sorted, name)
	}
	sort.Strings(sorted)

	ents := make([]*entity, len(sorted))
	for i, name := range sorted {
		ents[i] = e.entity(name, true)
		ents[i].mu.Lock()
	}

	if typeOps {
		e.typesMu.Lock()
	}

	if tx.clear {
		e.kv.lockAll()
	}

	defer func() {

		if tx.clear {
			e.kv.unlockAll()
		}

		if typeOps {
			e.typesMu.Unlock()
		}

		for i := len(ents) - 1; i >= 0; i-- {
			ents[i].mu.Unlock()
		}
	}()

	if tx.clear {
		for _, ent := range e.allEntities() {
			if !names[ent.name] {
				return applied{}, ErrConflict
			}
		}
	}

	c := &commitCtx{e: e, drafts: map[string]*draft{}}
	for key, lsn := range tx.reads {
		entity, id := splitDocRef(key)
		current := uint64(0)
		if names[entity] {
			st, err := c.draft(entity).lookup(id)
			if err != nil {
				return applied{}, err
			}
			if st.exists {
				current = st.lsn
			}
		} else {
			r, err := e.readLatest(entity, id)
			if err != nil {
				return applied{}, err
			}
			current = r.lsn
		}

		if current != lsn {
			return applied{}, ErrConflict
		}
	}

	c.b = e.db.NewBatch()
	c.lsn = e.lsn.Add(1)
	types := e.Types()
	typesChanged := false
	kvCleared := false

	fail := func(err error) (applied, error) {
		_ = c.b.Close()
		return applied{}, err
	}

	for i := range tx.ops {
		op := &tx.ops[i]
		var err error
		switch op.kind {
		case opPut:
			err = c.draft(op.entity).put(op.id, op.value, op.doc)
		case opDelete:
			err = c.draft(op.entity).del(op.id)
		case opReset:
			err = c.draft(op.entity).resetEntity()
		case opCreateType:
			if !containsString(types, op.entity) {
				types = append(types, op.entity)
				typesChanged = true
			}
		case opDropType:
			if containsString(types, op.entity) {
				types = removeString(types, op.entity)
				typesChanged = true
			}
		case opClearAll:
			if err = e.kv.clear(c.b); err != nil {
				break
			}
			kvCleared = true
			for _, name := range sorted {
				if err = c.draft(name).resetEntity(); err != nil {
					break
				}
			}
			if len(types) > 0 {
				types = nil
				typesChanged = true
			}
		}

		if err != nil {
			return fail(err)
		}
	}

	for _, d := range c.order {
		if !d.changed {
			continue
		}

		d.rec.version = c.lsn
		if err := c.b.Set(entityMetaKey(d.ent.name), encodeRecord(d.rec), nil); err != nil {
			return fail(err)
		}
	}

	if typesChanged {
		if err := c.b.Set(metaKey(metaTypes), encodeTypes(types), nil); err != nil {
			return fail(err)
		}
	}

	if c.b.Empty() {
		_ = c.b.Close()
		return applied{}, nil
	}

	a, err := e.apply(c.b, true)
	if err != nil {
		return applied{}, err
	}

	for _, d := range c.order {
		if !d.changed {
			continue
		}

		if d.reset && d.ent.job != nil {
			d.ent.job.canceled = true
			d.ent.job = nil
		}

		if job := d.ent.job; job != nil {
			for seq := range d.dirty {
				job.dirty.Set(seq)
			}
		}

		d.ent.rec.Store(d.rec)
		for _, r := range d.cleared {
			e.scheduleCompaction(r)
		}
	}

	if typesChanged {
		e.catalogMu.Lock()
		e.types = types
		e.catalogMu.Unlock()
	}

	if kvCleared {
		e.kv.resetCounts()
	}

	return a, nil
}

func containsString(list []string, s string) bool {
	for _, x := range list {
		if x == s {
			return true
		}
	}

	return false
}

func removeString(list []string, s string) []string {
	out := make([]string, 0, len(list))
	for _, x := range list {
		if x != s {
			out = append(out, x)
		}
	}

	return out
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
