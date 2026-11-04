package pagetree

import (
	"fmt"
)

type Loader interface {
	Load(ref *Ref) (*Page, error)
}

type Tree struct {
	root *Ref
	ld   Loader
	tok  *Token
}

func New(ld Loader) Tree {
	return Tree{ld: ld}
}

func Open(ld Loader, root *Ref) Tree {
	return Tree{root: root, ld: ld}
}

func (t Tree) WithToken(tok *Token) Tree {
	t.tok = tok
	return t
}

func (t Tree) Root() *Ref {
	return t.root
}

func (t Tree) Loader() Loader {
	return t.ld
}

func (t Tree) page(r *Ref) *Page {
	if p := r.mem.Load(); p != nil {
		return p
	}

	if t.ld == nil {
		panic(fmt.Sprintf("pagetree: page %d is not in memory and the tree has no loader", r.ID()))
	}

	p, err := t.ld.Load(r)
	if err != nil {
		panic(fmt.Sprintf("pagetree: cannot load page %d: %v", r.ID(), err))
	}

	return p
}

func (t Tree) Page(r *Ref) *Page {
	return t.page(r)
}

func (t Tree) own(r *Ref, p *Page) (*Page, *Ref) {
	if t.tok.active() && p.owner == t.tok {
		return p, r
	}

	c := p.clone(t.tok)

	return c, NewRef(c)
}

func (t Tree) fresh(p *Page) *Ref {
	p.owner = t.tok
	return NewRef(p)
}

func (t Tree) Len() int {
	if t.root == nil {
		return 0
	}

	return int(t.page(t.root).total)
}

func (t Tree) Get(key string) ([]byte, bool) {
	if t.root == nil {
		return nil, false
	}

	k := []byte(key)
	p := t.page(t.root)
	for !p.leaf {
		p = t.page(p.kids[p.childIndex(k)])
	}

	if i, ok := p.search(k); ok {
		return p.val(i), true
	}

	return nil, false
}

func (t Tree) Put(key string, val []byte) (Tree, bool) {
	k := []byte(key)
	if t.root == nil {
		p := emptyLeaf(t.tok)
		p.insertKey(0, k)
		p.insertVal(0, val)
		p.total = 1
		t.root = t.fresh(p)
		return t, false
	}

	ref, split, replaced := t.put(t.root, k, val)
	if split == nil {
		t.root = ref
		return t, replaced
	}

	left, right := t.page(ref), t.page(split)
	root := &Page{owner: t.tok, koff: []uint32{0}}
	root.insertKey(0, left.minKey())
	root.insertKey(1, right.minKey())
	root.kids = []*Ref{ref, split}
	root.sizes = []uint64{left.total, right.total}
	root.total = left.total + right.total
	t.root = t.fresh(root)

	return t, replaced
}

func (t Tree) put(r *Ref, key, val []byte) (*Ref, *Ref, bool) {
	p := t.page(r)
	if p.leaf {
		i, found := p.search(key)
		c, cr := t.own(r, p)
		if found {
			c.setVal(i, val)
			return cr, nil, true
		}

		c.insertKey(i, key)
		c.insertVal(i, val)
		c.total++
		if !c.full() {
			return cr, nil, false
		}

		right := c.split(t.tok)

		return cr, t.fresh(right), false
	}

	i := p.childIndex(key)
	kid, split, replaced := t.put(p.kids[i], key, val)

	c, cr := t.own(r, p)
	kp := t.page(kid)
	c.kids[i] = kid
	c.sizes[i] = kp.total
	c.setKey(i, kp.minKey())
	if !replaced {
		c.total++
	}

	if split != nil {
		sp := t.page(split)
		c.insertKey(i+1, sp.minKey())
		c.kids = insertAt(c.kids, i+1, split)
		c.sizes = insertAt(c.sizes, i+1, sp.total)
	}

	if !c.full() {
		return cr, nil, replaced
	}

	right := c.split(t.tok)

	return cr, t.fresh(right), replaced
}

func (t Tree) Delete(key string) (Tree, bool) {
	if t.root == nil {
		return t, false
	}

	ref, removed := t.del(t.root, []byte(key))
	if !removed {
		return t, false
	}

	if ref == nil {
		t.root = nil
		return t, true
	}

	p := t.page(ref)
	if p.total == 0 {
		t.root = nil
		return t, true
	}

	for !p.leaf && len(p.kids) == 1 {
		ref = p.kids[0]
		p = t.page(ref)
	}

	t.root = ref

	return t, true
}

func (t Tree) del(r *Ref, key []byte) (*Ref, bool) {
	p := t.page(r)
	if p.leaf {
		i, found := p.search(key)
		if !found {
			return r, false
		}

		if p.Len() == 1 {
			return nil, true
		}

		c, cr := t.own(r, p)
		c.removeKey(i)
		c.removeVal(i)
		c.total--

		return cr, true
	}

	i := p.childIndex(key)
	kid, removed := t.del(p.kids[i], key)
	if !removed {
		return r, false
	}

	c, cr := t.own(r, p)
	c.total--

	if kid == nil {
		if len(c.kids) == 1 {
			return nil, true
		}

		c.removeKey(i)
		c.kids = removeAt(c.kids, i)
		c.sizes = removeAt(c.sizes, i)

		return cr, true
	}

	kp := t.page(kid)
	c.kids[i] = kid
	c.sizes[i] = kp.total
	c.setKey(i, kp.minKey())

	if kp.Len() < minEntries {
		t.merge(c, i)
	}

	return cr, true
}

func (t Tree) merge(c *Page, i int) {
	j := i + 1
	if j >= len(c.kids) {
		j = i - 1
	}

	if j < 0 {
		return
	}

	a, b := i, j
	if a > b {
		a, b = b, a
	}

	left, right := t.page(c.kids[a]), t.page(c.kids[b])
	if left.Len()+right.Len() > maxEntries || len(left.keys)+len(left.vals)+len(right.keys)+len(right.vals) > maxBytes {
		return
	}

	merged := left.clone(t.tok)
	base := uint32(len(merged.keys))
	merged.keys = append(merged.keys, right.keys...)
	for _, o := range right.koff[1:] {
		merged.koff = append(merged.koff, o+base)
	}

	if left.leaf {
		vbase := uint32(len(merged.vals))
		merged.vals = append(merged.vals, right.vals...)
		for _, o := range right.voff[1:] {
			merged.voff = append(merged.voff, o+vbase)
		}
	} else {
		merged.kids = append(merged.kids, right.kids...)
		merged.sizes = append(merged.sizes, right.sizes...)
	}
	merged.total = left.total + right.total

	c.kids[a] = t.fresh(merged)
	c.sizes[a] = merged.total
	c.setKey(a, merged.minKey())
	c.removeKey(b)
	c.kids = removeAt(c.kids, b)
	c.sizes = removeAt(c.sizes, b)
}

func (t Tree) Rank(key string) int {
	if t.root == nil {
		return 0
	}

	k := []byte(key)
	rank := uint64(0)
	p := t.page(t.root)
	for !p.leaf {
		i := p.childIndex(k)
		for j := 0; j < i; j++ {
			rank += p.sizes[j]
		}
		p = t.page(p.kids[i])
	}

	i, _ := p.search(k)

	return int(rank) + i
}

func (t Tree) At(index int) (string, []byte, bool) {
	if t.root == nil || index < 0 {
		return "", nil, false
	}

	p := t.page(t.root)
	if uint64(index) >= p.total {
		return "", nil, false
	}

	idx := uint64(index)
	for !p.leaf {
		i := 0
		for ; i < len(p.sizes)-1 && idx >= p.sizes[i]; i++ {
			idx -= p.sizes[i]
		}
		p = t.page(p.kids[i])
	}

	return str(p.key(int(idx))), p.val(int(idx)), true
}

func (t Tree) AscendAt(index int, fn func(key string, val []byte) bool) {
	if t.root == nil || index >= t.Len() {
		return
	}

	t.ascendAt(t.root, uint64(max(index, 0)), fn)
}

func (t Tree) ascendAt(r *Ref, index uint64, fn func(string, []byte) bool) bool {
	p := t.page(r)
	if p.leaf {
		for i := int(index); i < p.Len(); i++ {
			if !fn(str(p.key(i)), p.val(i)) {
				return false
			}
		}
		return true
	}

	for i := 0; i < len(p.kids); i++ {
		if index >= p.sizes[i] {
			index -= p.sizes[i]
			continue
		}

		if !t.ascendAt(p.kids[i], index, fn) {
			return false
		}
		index = 0
	}

	return true
}

func (t Tree) AscendFrom(key string, fn func(key string, val []byte) bool) {
	if t.root == nil {
		return
	}

	t.ascendFrom(t.root, []byte(key), fn)
}

func (t Tree) ascendFrom(r *Ref, key []byte, fn func(string, []byte) bool) bool {
	p := t.page(r)
	if p.leaf {
		i, _ := p.search(key)
		for ; i < p.Len(); i++ {
			if !fn(str(p.key(i)), p.val(i)) {
				return false
			}
		}
		return true
	}

	for i := p.childIndex(key); i < len(p.kids); i++ {
		if !t.ascendFrom(p.kids[i], key, fn) {
			return false
		}
	}

	return true
}

func (t Tree) DescendBefore(key string, fn func(key string, val []byte) bool) {
	if t.root == nil {
		return
	}

	t.descendBefore(t.root, []byte(key), fn)
}

func (t Tree) descendBefore(r *Ref, key []byte, fn func(string, []byte) bool) bool {
	p := t.page(r)
	if p.leaf {
		i, _ := p.search(key)
		for i--; i >= 0; i-- {
			if !fn(str(p.key(i)), p.val(i)) {
				return false
			}
		}
		return true
	}

	for i := p.childIndex(key); i >= 0; i-- {
		if !t.descendBefore(p.kids[i], key, fn) {
			return false
		}
	}

	return true
}

func (t Tree) DescendAt(index int, fn func(key string, val []byte) bool) {
	if t.root == nil || index < 0 {
		return
	}

	p := t.page(t.root)
	if uint64(index) >= p.total {
		index = int(p.total) - 1
	}

	t.descendAt(t.root, uint64(index), fn)
}

func (t Tree) descendAt(r *Ref, index uint64, fn func(string, []byte) bool) bool {
	p := t.page(r)
	if p.leaf {
		for i := int(index); i >= 0; i-- {
			if !fn(str(p.key(i)), p.val(i)) {
				return false
			}
		}
		return true
	}

	i := 0
	for ; i < len(p.sizes)-1 && index >= p.sizes[i]; i++ {
		index -= p.sizes[i]
	}

	for ; i >= 0; i-- {
		if !t.descendAt(p.kids[i], index, fn) {
			return false
		}
		if i > 0 {
			index = p.sizes[i-1] - 1
		}
	}

	return true
}

func FromSorted(ld Loader, keys []string, vals [][]byte) Tree {
	t := Tree{ld: ld}
	if len(keys) == 0 {
		return t
	}

	per := maxEntries * 3 / 4
	var level []*Ref
	for start := 0; start < len(keys); {
		p := emptyLeaf(nil)
		for i := start; i < len(keys) && p.Len() < per && len(p.keys)+len(p.vals) < maxBytes/2; i++ {
			p.keys = append(p.keys, keys[i]...)
			p.koff = append(p.koff, uint32(len(p.keys)))
			if vals != nil {
				p.vals = append(p.vals, vals[i]...)
			}
			p.voff = append(p.voff, uint32(len(p.vals)))
		}
		p.total = uint64(p.Len())
		start += p.Len()
		level = append(level, NewRef(p))
	}

	for len(level) > 1 {
		var next []*Ref
		for start := 0; start < len(level); start += per {
			end := min(start+per, len(level))
			p := &Page{koff: []uint32{0}}
			for _, kid := range level[start:end] {
				kp := kid.mem.Load()
				p.keys = append(p.keys, kp.minKey()...)
				p.koff = append(p.koff, uint32(len(p.keys)))
				p.kids = append(p.kids, kid)
				p.sizes = append(p.sizes, kp.total)
				p.total += kp.total
			}
			next = append(next, NewRef(p))
		}
		level = next
	}

	t.root = level[0]

	return t
}

func (t Tree) Equal(o Tree) bool {
	return t.root == o.root
}

func (t Tree) ForEachDirty(fn func(ref *Ref, p *Page) error) error {
	if t.root == nil {
		return nil
	}

	return forEachDirty(t.root, fn)
}

func forEachDirty(r *Ref, fn func(*Ref, *Page) error) error {
	if r.ID() != 0 {
		return nil
	}

	p := r.mem.Load()
	if p == nil {
		return fmt.Errorf("pagetree: an unwritten page is not in memory")
	}

	if !p.leaf {
		for _, kid := range p.kids {
			if err := forEachDirty(kid, fn); err != nil {
				return err
			}
		}
	}

	return fn(r, p)
}

func (t Tree) height() int {
	h := 1
	p := t.page(t.root)
	for !p.leaf {
		p = t.page(p.kids[0])
		h++
	}

	return h
}

func (t Tree) WalkStored(fn func(id uint64, size uint32)) {
	if t.root == nil {
		return
	}

	t.walkStored(t.root, t.height(), fn)
}

func (t Tree) walkStored(r *Ref, level int, fn func(uint64, uint32)) {
	if id := r.ID(); id != 0 {
		fn(id, r.Size())
	}

	if level == 1 {
		return
	}

	p := t.page(r)
	for _, kid := range p.kids {
		t.walkStored(kid, level-1, fn)
	}
}

func (t Tree) Relocate(victim func(id uint64) bool) (Tree, int) {
	if t.root == nil {
		return t, 0
	}

	moved := 0
	t.root = t.relocate(t.root, t.height(), victim, &moved)

	return t, moved
}

func (t Tree) relocate(r *Ref, level int, victim func(uint64) bool, moved *int) *Ref {
	isVictim := r.ID() != 0 && victim(r.ID())
	if level == 1 && !isVictim {
		return r
	}

	p := t.page(r)
	var c *Page
	cr := r
	if isVictim {
		c, cr = t.own(r, p)
		*moved++
	}

	if level > 1 {
		for i, kid := range p.kids {
			nk := t.relocate(kid, level-1, victim, moved)
			if nk == kid {
				continue
			}
			if c == nil {
				c, cr = t.own(r, p)
			}
			c.kids[i] = nk
		}
	}

	return cr
}
