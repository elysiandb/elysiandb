package datastore

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"runtime"
	"sync"
	"sync/atomic"
)

var errMissing = errors.New("datastore: document not found")

type entity struct {
	name   string
	mu     sync.Mutex
	rec    atomic.Pointer[entityRecord]
	job    *buildJob
	queued atomic.Bool
	usage  sync.Map
}

func newEntity(name string) *entity {
	return &entity{name: name}
}

type Snapshot struct {
	e       *Engine
	name    string
	id      uint32
	p       *pin
	cleanup runtime.Cleanup
	freed   atomic.Bool
	once    sync.Once
	rec     *entityRecord
}

func (e *Engine) Snapshot(name string) *Snapshot {
	s := &Snapshot{e: e, name: name}
	if e.closed.Load() {
		return s
	}

	ent := e.entity(name, false)
	if ent == nil {
		return s
	}

	rec := ent.rec.Load()
	if rec == nil {
		return s
	}

	p := e.acquirePin()
	if p == nil {
		return s
	}

	s.id = rec.id
	s.attach(p)

	return s
}

func (s *Snapshot) attach(p *pin) {
	s.p = p
	s.cleanup = runtime.AddCleanup(s, func(p *pin) { p.release() }, p)
}

func (s *Snapshot) Release() {
	if s.p == nil || !s.freed.CompareAndSwap(false, true) {
		return
	}

	s.cleanup.Stop()
	s.p.release()
}

func (e *Engine) SnapshotAll() map[string]*Snapshot {
	out := map[string]*Snapshot{}
	if e.closed.Load() {
		return out
	}

	p := e.acquirePin()
	if p == nil {
		return out
	}
	defer p.release()

	for _, ent := range e.allEntities() {
		rec := ent.rec.Load()
		if rec == nil {
			continue
		}

		s := &Snapshot{e: e, name: ent.name, id: rec.id}
		p.refs.Add(1)
		s.attach(p)
		if s.record() == nil {
			s.Release()
			continue
		}
		out[ent.name] = s
	}

	return out
}

func (s *Snapshot) record() *entityRecord {
	if s.p == nil {
		return nil
	}

	defer runtime.KeepAlive(s)

	s.once.Do(func() {
		b, ok, err := s.p.get(entityMetaKey(s.name))
		if err != nil || !ok {
			return
		}

		rec, err := decodeRecord(b)
		if err == nil {
			s.rec = rec
		}
	})

	return s.rec
}

func (s *Snapshot) Name() string {
	return s.name
}

func (s *Snapshot) Len() int {
	if r := s.record(); r != nil {
		return int(r.count)
	}

	return 0
}

func (s *Snapshot) Version() uint64 {
	if r := s.record(); r != nil {
		return r.version
	}

	return 0
}

func (s *Snapshot) Generation() uint64 {
	if r := s.record(); r != nil {
		return r.gen
	}

	return 0
}

func (s *Snapshot) NextSeq() uint32 {
	if r := s.record(); r != nil {
		return r.nextSeq
	}

	return 0
}

func (s *Snapshot) Refs() []RefPath {
	if r := s.record(); r != nil {
		return r.refs
	}

	return nil
}

type cursor struct {
	pos   uint64
	value []byte
}

type Doc struct {
	s   *Snapshot
	seq uint32
	cur *cursor
	pos uint64
	rec []byte
}

func (d Doc) record() ([]byte, error) {
	if d.rec != nil {
		return d.rec, nil
	}

	if d.cur != nil && d.cur.pos == d.pos {
		return d.cur.value, nil
	}

	if d.s == nil || d.s.p == nil {
		return nil, errMissing
	}
	defer runtime.KeepAlive(d.s)

	b, ok, err := d.s.p.get(docKey(d.s.id, d.seq))
	if err != nil {
		return nil, err
	}

	if !ok {
		return nil, errMissing
	}

	return b, nil
}

func (d Doc) Detach() Doc {
	if d.rec != nil {
		return d
	}

	b, err := d.record()
	if err != nil {
		return d
	}

	return Doc{s: d.s, seq: d.seq, rec: append([]byte(nil), b...)}
}

func (d Doc) ID() string {
	b, err := d.record()
	if err != nil {
		return ""
	}

	_, id, _, ok := decodeDocValue(b)
	if !ok {
		return ""
	}

	return string(id)
}

func (d Doc) Seq() uint32 {
	return d.seq
}

func (d Doc) LSN() uint64 {
	b, err := d.record()
	if err != nil {
		return 0
	}

	lsn, _, _, _ := decodeDocValue(b)

	return lsn
}

func (d Doc) raw() ([]byte, error) {
	b, err := d.record()
	if err != nil {
		return nil, err
	}

	_, _, value, ok := decodeDocValue(b)
	if !ok {
		return nil, errRecord
	}

	return value, nil
}

func (d Doc) Bytes() ([]byte, error) {
	b, err := d.raw()
	if err != nil {
		return nil, err
	}

	return append([]byte(nil), b...), nil
}

func (d Doc) Decode() (map[string]any, error) {
	b, err := d.raw()
	if err != nil {
		return nil, err
	}

	doc := decodeDoc(b)
	if doc == nil {
		return nil, fmt.Errorf("document %s is not a JSON object", d.ID())
	}

	return doc, nil
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

func (s *Snapshot) Indexed(seq uint32) Doc {
	return Doc{s: s, seq: seq}
}

func (s *Snapshot) Get(id string) (Doc, bool) {
	if s.p == nil {
		return Doc{}, false
	}
	defer runtime.KeepAlive(s)

	sb, ok, err := s.p.get(idKey(s.id, id))
	if err != nil || !ok || len(sb) != 4 {
		return Doc{}, false
	}

	seq := binary.BigEndian.Uint32(sb)
	rec, ok, err := s.p.get(docKey(s.id, seq))
	if err != nil || !ok {
		return Doc{}, false
	}

	if _, got, _, valid := decodeDocValue(rec); !valid || string(got) != id {
		return Doc{}, false
	}

	return Doc{s: s, seq: seq, rec: rec}, true
}

func (s *Snapshot) BySeq(seq uint32) (Doc, bool) {
	if s.p == nil {
		return Doc{}, false
	}
	defer runtime.KeepAlive(s)

	rec, ok, err := s.p.get(docKey(s.id, seq))
	if err != nil || !ok {
		return Doc{}, false
	}

	return Doc{s: s, seq: seq, rec: rec}, true
}

func (s *Snapshot) Ascend(offset int, fn func(Doc) bool) {
	if s.p == nil || s.Len() == 0 {
		return
	}

	lower := partPrefix(s.id, partDocs)
	it, err := s.p.iter(lower, prefixEnd(lower))
	if err != nil {
		return
	}

	cur := &cursor{}
	defer func() {
		cur.pos++
		cur.value = nil
		_ = it.Close()
	}()

	n := 0
	for it.First(); it.Valid(); it.Next() {
		if n < offset {
			n++
			continue
		}

		key := it.Key()
		if len(key) != 10 {
			continue
		}

		cur.pos++
		cur.value = it.Value()
		if !fn(Doc{s: s, seq: binary.BigEndian.Uint32(key[6:]), cur: cur, pos: cur.pos}) {
			return
		}
	}
}

func (s *Snapshot) Index(spec IndexSpec) (*IndexView, bool) {
	r := s.record()
	m := r.index(spec.Name())
	if m == nil || m.state != stateReady {
		return nil, false
	}

	if ent := s.e.entity(s.name, false); ent != nil {
		ent.touch(m.name)
	}

	return &IndexView{s: s, meta: m, prefix: indexPrefix(r.id, m.no)}, true
}

func (s *Snapshot) HasIndex(spec IndexSpec) bool {
	return s.record().index(spec.Name()) != nil
}

func (s *Snapshot) IndexDormant(spec IndexSpec) bool {
	m := s.record().index(spec.Name())
	return m != nil && m.state == stateDormant
}

type IndexView struct {
	s      *Snapshot
	meta   *indexMeta
	prefix []byte
}

func concat(a, b []byte) []byte {
	out := make([]byte, 0, len(a)+len(b))
	out = append(out, a...)

	return append(out, b...)
}

func keySeq(k []byte) uint32 {
	return binary.BigEndian.Uint32(k[len(k)-4:])
}

func (v *IndexView) Len() int {
	return int(v.meta.entries)
}

func (v *IndexView) CountKind(kind byte) int {
	return v.meta.countKind(kind)
}

func (v *IndexView) CountBelow(kind byte) int {
	return v.meta.countBelow(kind)
}

func (v *IndexView) scan(lower, upper []byte, reverse bool, fn func(entry []byte) bool) {
	defer runtime.KeepAlive(v.s)

	it, err := v.s.p.iter(lower, upper)
	if err != nil {
		return
	}
	defer it.Close()

	n := len(v.prefix)
	if reverse {
		for it.Last(); it.Valid(); it.Prev() {
			if k := it.Key(); len(k) >= n+4 && !fn(k[n:]) {
				return
			}
		}
		return
	}

	for it.First(); it.Valid(); it.Next() {
		if k := it.Key(); len(k) >= n+4 && !fn(k[n:]) {
			return
		}
	}
}

func (v *IndexView) bounds(lo, hi []byte) ([]byte, []byte) {
	lower := concat(v.prefix, lo)
	upper := prefixEnd(v.prefix)
	if hi != nil {
		upper = concat(v.prefix, hi)
	}

	return lower, upper
}

func (v *IndexView) ScanRange(lo, hi []byte, fn func(key string, seq uint32) bool) {
	lower, upper := v.bounds(lo, hi)
	v.scan(lower, upper, false, func(k []byte) bool {
		return fn(string(k), keySeq(k))
	})
}

func (v *IndexView) ScanSeqs(lo, hi []byte, fn func(seq uint32) bool) {
	lower, upper := v.bounds(lo, hi)
	v.scan(lower, upper, false, func(k []byte) bool {
		return fn(keySeq(k))
	})
}

func (v *IndexView) ScanPrefix(prefix []byte, fn func(key string, seq uint32) bool) {
	lower := concat(v.prefix, prefix)
	v.scan(lower, prefixEnd(lower), false, func(k []byte) bool {
		return fn(string(k), keySeq(k))
	})
}

func (v *IndexView) Walk(ascending bool, skip int, fn func(seq uint32) bool) {
	if ascending {
		v.scan(v.prefix, prefixEnd(v.prefix), false, func(k []byte) bool {
			if skip > 0 {
				skip--
				return true
			}
			return fn(keySeq(k))
		})
		return
	}

	missing := concat(v.prefix, []byte{SortRankMissing})
	stopped := false
	v.walkGroupsReverse(v.prefix, missing, func(seqs []uint32) bool {
		if skip >= len(seqs) {
			skip -= len(seqs)
			return true
		}

		for _, seq := range seqs[skip:] {
			if !fn(seq) {
				stopped = true
				return false
			}
		}
		skip = 0

		return true
	})

	if stopped {
		return
	}

	v.scan(missing, prefixEnd(v.prefix), false, func(k []byte) bool {
		if skip > 0 {
			skip--
			return true
		}
		return fn(keySeq(k))
	})
}

func (v *IndexView) walkGroupsReverse(lower, upper []byte, fn func(seqs []uint32) bool) {
	var payload []byte
	var group []uint32
	flush := func() bool {
		if len(group) == 0 {
			return true
		}

		for i, j := 0, len(group)-1; i < j; i, j = i+1, j-1 {
			group[i], group[j] = group[j], group[i]
		}

		ok := fn(group)
		group = group[:0]

		return ok
	}

	stopped := false
	v.scan(lower, upper, true, func(k []byte) bool {
		p := k[:len(k)-4]
		if len(group) > 0 && string(p) != string(payload) {
			if !flush() {
				stopped = true
				return false
			}
		}

		if len(group) == 0 {
			payload = append(payload[:0], p...)
		}
		group = append(group, keySeq(k))

		return true
	})

	if !stopped {
		flush()
	}
}

func (v *IndexView) walkGroupsForward(lower, upper []byte, fn func(seqs []uint32) bool) bool {
	var payload []byte
	var group []uint32
	stopped := false
	v.scan(lower, upper, false, func(k []byte) bool {
		p := k[:len(k)-4]
		if len(group) > 0 && string(p) != string(payload) {
			if !fn(group) {
				stopped = true
				return false
			}
			group = group[:0]
		}

		if len(group) == 0 {
			payload = append(payload[:0], p...)
		}
		group = append(group, keySeq(k))

		return true
	})

	if stopped {
		return false
	}

	if len(group) > 0 {
		return fn(group)
	}

	return true
}

func (v *IndexView) WalkGroups(ascending bool, fn func(seqs []uint32) bool) {
	missing := concat(v.prefix, []byte{SortRankMissing})
	if ascending {
		if !v.walkGroupsForward(v.prefix, missing, fn) {
			return
		}
	} else {
		stopped := false
		v.walkGroupsReverse(v.prefix, missing, func(seqs []uint32) bool {
			if !fn(seqs) {
				stopped = true
				return false
			}
			return true
		})
		if stopped {
			return
		}
	}

	var rest []uint32
	v.scan(missing, prefixEnd(v.prefix), false, func(k []byte) bool {
		rest = append(rest, keySeq(k))
		return true
	})

	if len(rest) > 0 {
		fn(rest)
	}
}
