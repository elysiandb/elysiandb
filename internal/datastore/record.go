package datastore

import (
	"encoding/binary"
	"errors"
	"sort"
)

type indexState byte

const (
	stateBuilding indexState = 1
	stateReady    indexState = 2
	stateDormant  indexState = 3
)

const recordVersion = 1

var errRecord = errors.New("datastore: corrupt entity record")

type kindCount struct {
	kind byte
	n    int64
}

type indexMeta struct {
	name    string
	spec    IndexSpec
	no      uint16
	state   indexState
	entries int64
	kinds   []kindCount
}

func (m *indexMeta) clone() *indexMeta {
	c := *m
	c.kinds = append([]kindCount(nil), m.kinds...)

	return &c
}

func (m *indexMeta) counted() bool {
	return m.spec.Kind == IndexFilter || m.spec.Kind == IndexSort
}

func (m *indexMeta) addKind(kind byte, delta int64) {
	i := sort.Search(len(m.kinds), func(i int) bool { return m.kinds[i].kind >= kind })
	if i < len(m.kinds) && m.kinds[i].kind == kind {
		m.kinds[i].n += delta
		if m.kinds[i].n <= 0 {
			m.kinds = append(m.kinds[:i], m.kinds[i+1:]...)
		}
		return
	}

	if delta <= 0 {
		return
	}

	m.kinds = append(m.kinds, kindCount{})
	copy(m.kinds[i+1:], m.kinds[i:])
	m.kinds[i] = kindCount{kind: kind, n: delta}
}

func (m *indexMeta) countKind(kind byte) int {
	for _, k := range m.kinds {
		if k.kind == kind {
			return int(k.n)
		}
	}

	return 0
}

func (m *indexMeta) countBelow(kind byte) int {
	n := int64(0)
	for _, k := range m.kinds {
		if k.kind < kind {
			n += k.n
		}
	}

	return int(n)
}

func (m *indexMeta) clear() {
	m.entries = 0
	m.kinds = nil
}

type entityRecord struct {
	id          uint32
	gen         uint64
	nextSeq     uint32
	count       int64
	version     uint64
	nextIndexNo uint16
	refs        []RefPath
	indexes     []*indexMeta
}

func (r *entityRecord) clone() *entityRecord {
	c := *r
	c.refs = append([]RefPath(nil), r.refs...)
	c.indexes = make([]*indexMeta, len(r.indexes))
	for i, m := range r.indexes {
		c.indexes[i] = m.clone()
	}

	return &c
}

func (r *entityRecord) index(name string) *indexMeta {
	if r == nil {
		return nil
	}

	i := sort.Search(len(r.indexes), func(i int) bool { return r.indexes[i].name >= name })
	if i < len(r.indexes) && r.indexes[i].name == name {
		return r.indexes[i]
	}

	return nil
}

func (r *entityRecord) addIndex(spec IndexSpec, state indexState) *indexMeta {
	m := &indexMeta{name: spec.Name(), spec: spec, no: r.nextIndexNo, state: state}
	r.nextIndexNo++
	i := sort.Search(len(r.indexes), func(i int) bool { return r.indexes[i].name >= m.name })
	r.indexes = append(r.indexes, nil)
	copy(r.indexes[i+1:], r.indexes[i:])
	r.indexes[i] = m

	return m
}

func (r *entityRecord) hasRef(ref RefPath) bool {
	for _, x := range r.refs {
		if x == ref {
			return true
		}
	}

	return false
}

func (r *entityRecord) addRef(ref RefPath) {
	i := sort.Search(len(r.refs), func(i int) bool {
		if r.refs[i].Path != ref.Path {
			return r.refs[i].Path >= ref.Path
		}
		return r.refs[i].Target >= ref.Target
	})
	r.refs = append(r.refs, RefPath{})
	copy(r.refs[i+1:], r.refs[i:])
	r.refs[i] = ref
}

func appendString(dst []byte, s string) []byte {
	dst = binary.AppendUvarint(dst, uint64(len(s)))
	return append(dst, s...)
}

func encodeRecord(r *entityRecord) []byte {
	b := make([]byte, 0, 64+32*len(r.indexes))
	b = append(b, recordVersion)
	b = binary.AppendUvarint(b, uint64(r.id))
	b = binary.AppendUvarint(b, r.gen)
	b = binary.AppendUvarint(b, uint64(r.nextSeq))
	b = binary.AppendVarint(b, r.count)
	b = binary.AppendUvarint(b, r.version)
	b = binary.AppendUvarint(b, uint64(r.nextIndexNo))
	b = binary.AppendUvarint(b, uint64(len(r.refs)))
	for _, ref := range r.refs {
		b = appendString(b, ref.Path)
		b = appendString(b, ref.Target)
	}

	b = binary.AppendUvarint(b, uint64(len(r.indexes)))
	for _, m := range r.indexes {
		b = appendString(b, m.name)
		b = binary.AppendUvarint(b, uint64(m.no))
		b = append(b, byte(m.state))
		b = binary.AppendVarint(b, m.entries)
		b = binary.AppendUvarint(b, uint64(len(m.kinds)))
		for _, k := range m.kinds {
			b = append(b, k.kind)
			b = binary.AppendVarint(b, k.n)
		}
	}

	return b
}

type recordReader struct {
	b   []byte
	pos int
	err error
}

func (rr *recordReader) uvarint() uint64 {
	if rr.err != nil {
		return 0
	}

	v, n := binary.Uvarint(rr.b[rr.pos:])
	if n <= 0 {
		rr.err = errRecord
		return 0
	}
	rr.pos += n

	return v
}

func (rr *recordReader) varint() int64 {
	if rr.err != nil {
		return 0
	}

	v, n := binary.Varint(rr.b[rr.pos:])
	if n <= 0 {
		rr.err = errRecord
		return 0
	}
	rr.pos += n

	return v
}

func (rr *recordReader) byte() byte {
	if rr.err != nil || rr.pos >= len(rr.b) {
		rr.err = errRecord
		return 0
	}

	c := rr.b[rr.pos]
	rr.pos++

	return c
}

func (rr *recordReader) str() string {
	l := rr.uvarint()
	if rr.err != nil || uint64(len(rr.b)-rr.pos) < l {
		rr.err = errRecord
		return ""
	}

	s := string(rr.b[rr.pos : rr.pos+int(l)])
	rr.pos += int(l)

	return s
}

func decodeRecord(b []byte) (*entityRecord, error) {
	if len(b) == 0 || b[0] != recordVersion {
		return nil, errRecord
	}

	rr := &recordReader{b: b, pos: 1}
	r := &entityRecord{}
	r.id = uint32(rr.uvarint())
	r.gen = rr.uvarint()
	r.nextSeq = uint32(rr.uvarint())
	r.count = rr.varint()
	r.version = rr.uvarint()
	r.nextIndexNo = uint16(rr.uvarint())
	for n := rr.uvarint(); n > 0 && rr.err == nil; n-- {
		r.refs = append(r.refs, RefPath{Path: rr.str(), Target: rr.str()})
	}

	for n := rr.uvarint(); n > 0 && rr.err == nil; n-- {
		m := &indexMeta{name: rr.str()}
		m.no = uint16(rr.uvarint())
		m.state = indexState(rr.byte())
		m.entries = rr.varint()
		for k := rr.uvarint(); k > 0 && rr.err == nil; k-- {
			m.kinds = append(m.kinds, kindCount{kind: rr.byte(), n: rr.varint()})
		}

		spec, ok := ParseIndexName(m.name)
		if !ok && rr.err == nil {
			rr.err = errRecord
		}
		m.spec = spec
		r.indexes = append(r.indexes, m)
	}

	if rr.err != nil {
		return nil, rr.err
	}

	return r, nil
}

func encodeTypes(types []string) []byte {
	b := binary.AppendUvarint(nil, uint64(len(types)))
	for _, t := range types {
		b = appendString(b, t)
	}

	return b
}

func decodeTypes(b []byte) ([]string, error) {
	rr := &recordReader{b: b}
	out := []string{}
	for n := rr.uvarint(); n > 0 && rr.err == nil; n-- {
		out = append(out, rr.str())
	}

	return out, rr.err
}
