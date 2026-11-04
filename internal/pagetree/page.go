package pagetree

import (
	"bytes"
	"encoding/binary"
	"errors"
	"sync/atomic"
	"unsafe"
)

const (
	maxEntries = 64
	minEntries = 16
	maxBytes   = 8 << 10
)

var ErrCorruptPage = errors.New("pagetree: corrupt page")

type Token struct {
	frozen bool
}

func NewToken() *Token {
	return &Token{}
}

func (t *Token) Freeze() {
	if t != nil {
		t.frozen = true
	}
}

func (t *Token) active() bool {
	return t != nil && !t.frozen
}

type Ref struct {
	id   atomic.Uint64
	size atomic.Uint32
	mem  atomic.Pointer[Page]
}

func NewRef(p *Page) *Ref {
	r := &Ref{}
	r.mem.Store(p)
	return r
}

func StoredRef(id uint64, size uint32) *Ref {
	r := &Ref{}
	r.id.Store(id)
	r.size.Store(size)
	return r
}

func (r *Ref) ID() uint64 {
	return r.id.Load()
}

func (r *Ref) Size() uint32 {
	return r.size.Load()
}

func (r *Ref) Resident() *Page {
	return r.mem.Load()
}

func (r *Ref) Persisted(id uint64, size uint32) {
	r.size.Store(size)
	r.id.Store(id)
}

func (r *Ref) Attach(p *Page) *Page {
	if r.mem.CompareAndSwap(nil, p) {
		return p
	}

	return r.mem.Load()
}

func (r *Ref) Evict() bool {
	if r.id.Load() == 0 {
		return false
	}

	return r.mem.Swap(nil) != nil
}

type Page struct {
	leaf  bool
	owner *Token
	keys  []byte
	koff  []uint32
	vals  []byte
	voff  []uint32
	kids  []*Ref
	sizes []uint64
	total uint64
}

func (p *Page) Leaf() bool {
	return p.leaf
}

func (p *Page) Len() int {
	return len(p.koff) - 1
}

func (p *Page) Total() uint64 {
	return p.total
}

func (p *Page) Kids() []*Ref {
	return p.kids
}

func (p *Page) Bytes() int {
	return len(p.keys) + len(p.vals) + 4*(len(p.koff)+len(p.voff)) + 24*len(p.kids) + 128
}

func (p *Page) key(i int) []byte {
	return p.keys[p.koff[i]:p.koff[i+1]]
}

func (p *Page) val(i int) []byte {
	return p.vals[p.voff[i]:p.voff[i+1]]
}

func (p *Page) minKey() []byte {
	return p.key(0)
}

func str(b []byte) string {
	if len(b) == 0 {
		return ""
	}

	return unsafe.String(&b[0], len(b))
}

func (p *Page) search(key []byte) (int, bool) {
	lo, hi := 0, p.Len()
	for lo < hi {
		m := int(uint(lo+hi) >> 1)
		if bytes.Compare(p.key(m), key) < 0 {
			lo = m + 1
		} else {
			hi = m
		}
	}

	return lo, lo < p.Len() && bytes.Equal(p.key(lo), key)
}

func (p *Page) childIndex(key []byte) int {
	lo, hi := 0, p.Len()
	for lo < hi {
		m := int(uint(lo+hi) >> 1)
		if bytes.Compare(p.key(m), key) > 0 {
			hi = m
		} else {
			lo = m + 1
		}
	}

	if lo == 0 {
		return 0
	}

	return lo - 1
}

func (p *Page) clone(owner *Token) *Page {
	c := &Page{
		leaf:  p.leaf,
		owner: owner,
		keys:  append(make([]byte, 0, len(p.keys)+64), p.keys...),
		koff:  append(make([]uint32, 0, len(p.koff)+2), p.koff...),
		total: p.total,
	}

	if p.leaf {
		c.vals = append(make([]byte, 0, len(p.vals)+64), p.vals...)
		c.voff = append(make([]uint32, 0, len(p.voff)+2), p.voff...)
		return c
	}

	c.kids = append(make([]*Ref, 0, len(p.kids)+2), p.kids...)
	c.sizes = append(make([]uint64, 0, len(p.sizes)+2), p.sizes...)

	return c
}

func insertBytes(b []byte, at int, x []byte) []byte {
	b = append(b, x...)
	copy(b[at+len(x):], b[at:len(b)-len(x)])
	copy(b[at:], x)

	return b
}

func removeBytes(b []byte, at, n int) []byte {
	copy(b[at:], b[at+n:])

	return b[:len(b)-n]
}

func insertOffset(off []uint32, i int, n int) []uint32 {
	off = append(off, 0)
	copy(off[i+1:], off[i:len(off)-1])
	for j := i + 1; j < len(off); j++ {
		off[j] += uint32(n)
	}

	return off
}

func removeOffset(off []uint32, i int) []uint32 {
	n := off[i+1] - off[i]
	copy(off[i+1:], off[i+2:])
	off = off[:len(off)-1]
	for j := i + 1; j < len(off); j++ {
		off[j] -= n
	}

	return off
}

func insertAt[T any](s []T, i int, v T) []T {
	var zero T
	s = append(s, zero)
	copy(s[i+1:], s[i:len(s)-1])
	s[i] = v

	return s
}

func removeAt[T any](s []T, i int) []T {
	copy(s[i:], s[i+1:])
	var zero T
	s[len(s)-1] = zero

	return s[:len(s)-1]
}

func (p *Page) insertKey(i int, k []byte) {
	p.keys = insertBytes(p.keys, int(p.koff[i]), k)
	p.koff = insertOffset(p.koff, i, len(k))
}

func (p *Page) removeKey(i int) {
	p.keys = removeBytes(p.keys, int(p.koff[i]), int(p.koff[i+1]-p.koff[i]))
	p.koff = removeOffset(p.koff, i)
}

func (p *Page) setKey(i int, k []byte) {
	if bytes.Equal(p.key(i), k) {
		return
	}

	p.removeKey(i)
	p.insertKey(i, k)
}

func (p *Page) insertVal(i int, v []byte) {
	p.vals = insertBytes(p.vals, int(p.voff[i]), v)
	p.voff = insertOffset(p.voff, i, len(v))
}

func (p *Page) removeVal(i int) {
	p.vals = removeBytes(p.vals, int(p.voff[i]), int(p.voff[i+1]-p.voff[i]))
	p.voff = removeOffset(p.voff, i)
}

func (p *Page) setVal(i int, v []byte) {
	cur := p.val(i)
	if len(cur) == len(v) {
		copy(cur, v)
		return
	}

	p.removeVal(i)
	p.insertVal(i, v)
}

func (p *Page) full() bool {
	return p.Len() > maxEntries || len(p.keys)+len(p.vals) > maxBytes && p.Len() > 2
}

func (p *Page) split(owner *Token) *Page {
	mid := p.Len() / 2
	right := &Page{leaf: p.leaf, owner: owner}

	kmid := p.koff[mid]
	right.keys = append([]byte(nil), p.keys[kmid:]...)
	right.koff = make([]uint32, 0, p.Len()-mid+1)
	for _, o := range p.koff[mid:] {
		right.koff = append(right.koff, o-kmid)
	}
	p.keys = p.keys[:kmid]
	p.koff = p.koff[:mid+1]

	if p.leaf {
		vmid := p.voff[mid]
		right.vals = append([]byte(nil), p.vals[vmid:]...)
		right.voff = make([]uint32, 0, len(p.voff)-mid)
		for _, o := range p.voff[mid:] {
			right.voff = append(right.voff, o-vmid)
		}
		p.vals = p.vals[:vmid]
		p.voff = p.voff[:mid+1]
		right.total = uint64(right.Len())
		p.total = uint64(p.Len())

		return right
	}

	right.kids = append([]*Ref(nil), p.kids[mid:]...)
	right.sizes = append([]uint64(nil), p.sizes[mid:]...)
	clear(p.kids[mid:])
	p.kids = p.kids[:mid]
	p.sizes = p.sizes[:mid]
	p.total, right.total = 0, 0
	for _, s := range p.sizes {
		p.total += s
	}
	for _, s := range right.sizes {
		right.total += s
	}

	return right
}

func emptyLeaf(owner *Token) *Page {
	return &Page{leaf: true, owner: owner, koff: []uint32{0}, voff: []uint32{0}}
}

func Encode(dst []byte, p *Page) ([]byte, error) {
	kind := byte(2)
	if p.leaf {
		kind = 1
	}
	dst = append(dst, kind)
	dst = binary.AppendUvarint(dst, uint64(p.Len()))

	for i := 0; i < p.Len(); i++ {
		k := p.key(i)
		dst = binary.AppendUvarint(dst, uint64(len(k)))
		dst = append(dst, k...)

		if p.leaf {
			v := p.val(i)
			dst = binary.AppendUvarint(dst, uint64(len(v)))
			dst = append(dst, v...)
			continue
		}

		id := p.kids[i].ID()
		if id == 0 {
			return nil, errors.New("pagetree: a child page is not written yet")
		}
		dst = binary.LittleEndian.AppendUint64(dst, id)
		dst = binary.AppendUvarint(dst, uint64(p.kids[i].Size()))
		dst = binary.AppendUvarint(dst, p.sizes[i])
	}

	return dst, nil
}

func Decode(b []byte) (*Page, error) {
	if len(b) < 2 || (b[0] != 1 && b[0] != 2) {
		return nil, ErrCorruptPage
	}

	p := &Page{leaf: b[0] == 1}
	pos := 1
	uv := func() (uint64, bool) {
		v, n := binary.Uvarint(b[pos:])
		if n <= 0 {
			return 0, false
		}
		pos += n
		return v, true
	}

	n, ok := uv()
	if !ok || n > 1<<16 {
		return nil, ErrCorruptPage
	}

	p.koff = make([]uint32, 1, n+1)
	p.keys = make([]byte, 0, len(b))
	if p.leaf {
		p.voff = make([]uint32, 1, n+1)
		p.vals = make([]byte, 0, len(b))
	} else {
		p.kids = make([]*Ref, 0, n)
		p.sizes = make([]uint64, 0, n)
	}

	for i := uint64(0); i < n; i++ {
		kl, ok := uv()
		if !ok || uint64(len(b)-pos) < kl {
			return nil, ErrCorruptPage
		}
		p.keys = append(p.keys, b[pos:pos+int(kl)]...)
		p.koff = append(p.koff, uint32(len(p.keys)))
		pos += int(kl)

		if p.leaf {
			vl, ok := uv()
			if !ok || uint64(len(b)-pos) < vl {
				return nil, ErrCorruptPage
			}
			p.vals = append(p.vals, b[pos:pos+int(vl)]...)
			p.voff = append(p.voff, uint32(len(p.vals)))
			pos += int(vl)
			continue
		}

		if len(b)-pos < 8 {
			return nil, ErrCorruptPage
		}
		id := binary.LittleEndian.Uint64(b[pos:])
		pos += 8
		sz, ok1 := uv()
		total, ok2 := uv()
		if !ok1 || !ok2 || id == 0 {
			return nil, ErrCorruptPage
		}
		p.kids = append(p.kids, StoredRef(id, uint32(sz)))
		p.sizes = append(p.sizes, total)
	}

	if pos != len(b) {
		return nil, ErrCorruptPage
	}

	if p.leaf {
		p.total = n
	} else {
		for _, s := range p.sizes {
			p.total += s
		}
	}

	return p, nil
}
