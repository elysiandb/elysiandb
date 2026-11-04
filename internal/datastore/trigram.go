package datastore

import (
	"encoding/binary"
	"math/bits"
	"slices"
	"sort"
	"strings"

	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/pagetree"
)

type Bitset struct {
	words []uint64
}

func NewBitset(capacity uint32) *Bitset {
	return &Bitset{words: make([]uint64, (capacity+63)/64)}
}

func (b *Bitset) Set(i uint32) {
	w := int(i / 64)
	if w >= len(b.words) {
		grown := make([]uint64, w+1+len(b.words)/2)
		copy(grown, b.words)
		b.words = grown
	}

	b.words[w] |= 1 << (i % 64)
}

func (b *Bitset) addContainer(chunk uint32, c []byte) {
	if len(c) == 0 {
		return
	}

	if c[0] != 1 {
		postingEach(c, chunk<<postingChunkBits, b.Set)
		return
	}

	first := int(chunk) * (postingChunk / 64)
	if need := first + postingChunk/64; need > len(b.words) {
		grown := make([]uint64, need+len(b.words)/2)
		copy(grown, b.words)
		b.words = grown
	}

	for w := 0; w < postingChunk/64; w++ {
		b.words[first+w] |= binary.LittleEndian.Uint64(c[1+8*w:])
	}
}

func (b *Bitset) Has(i uint32) bool {
	w := int(i / 64)
	return w < len(b.words) && b.words[w]&(1<<(i%64)) != 0
}

func (b *Bitset) And(o *Bitset) {
	for i := range b.words {
		if i < len(o.words) {
			b.words[i] &= o.words[i]
		} else {
			b.words[i] = 0
		}
	}
}

func (b *Bitset) AndNot(o *Bitset) {
	for i := range b.words {
		if i < len(o.words) {
			b.words[i] &^= o.words[i]
		}
	}
}

func (b *Bitset) Or(o *Bitset) {
	if len(o.words) > len(b.words) {
		grown := make([]uint64, len(o.words))
		copy(grown, b.words)
		b.words = grown
	}

	for i := range o.words {
		b.words[i] |= o.words[i]
	}
}

func (b *Bitset) Count() int {
	n := 0
	for _, w := range b.words {
		n += bits.OnesCount64(w)
	}

	return n
}

func (b *Bitset) Clone() *Bitset {
	return &Bitset{words: append([]uint64(nil), b.words...)}
}

func (b *Bitset) ForEach(fn func(i uint32) bool) {
	for wi, w := range b.words {
		for w != 0 {
			t := bits.TrailingZeros64(w)
			if !fn(uint32(wi*64 + t)) {
				return
			}
			w &= w - 1
		}
	}
}

func DocumentTrigrams(doc map[string]any) map[string]struct{} {
	out := map[string]struct{}{}
	var walk func(v any)
	walk = func(v any) {
		switch t := v.(type) {
		case map[string]any:
			for k, child := range t {
				if docpath.IsSearchableKey(k) {
					walk(child)
				}
			}
		case []any:
			for _, item := range t {
				walk(item)
			}
		case string:
			lower := strings.ToLower(t)
			for i := 0; i+3 <= len(lower); i++ {
				out[lower[i:i+3]] = struct{}{}
			}
		}
	}

	walk(doc)

	return out
}

func PatternTrigrams(pattern string) []string {
	lower := strings.ToLower(pattern)
	set := map[string]struct{}{}
	for _, run := range strings.Split(lower, "*") {
		if strings.Contains(run, "\\") {
			continue
		}

		for i := 0; i+3 <= len(run); i++ {
			set[run[i:i+3]] = struct{}{}
		}
	}

	out := make([]string, 0, len(set))
	for t := range set {
		out = append(out, t)
	}

	return out
}

const (
	staleSearchSlack = 1024
	postingChunkBits = 12
	postingChunk     = 1 << postingChunkBits
	postingArrayMax  = 256
	postingBitmap    = postingChunk / 8
)

func searchSpec() IndexSpec {
	return IndexSpec{Kind: IndexSearch}
}

func lowerASCII(t string) string {
	for i := 0; i < len(t); i++ {
		c := t[i]
		if c >= 0x80 || ('A' <= c && c <= 'Z') {
			return strings.ToLower(t)
		}
	}

	return t
}

func gramOf(s string, i int) uint32 {
	return uint32(s[i])<<16 | uint32(s[i+1])<<8 | uint32(s[i+2])
}

func searchGrams(doc map[string]any) []uint32 {
	if doc == nil {
		return nil
	}

	var out []uint32
	var walk func(v any)
	walk = func(v any) {
		switch t := v.(type) {
		case map[string]any:
			for k, child := range t {
				if docpath.IsSearchableKey(k) {
					walk(child)
				}
			}
		case []any:
			for _, item := range t {
				walk(item)
			}
		case string:
			lower := lowerASCII(t)
			for i := 0; i+3 <= len(lower); i++ {
				out = append(out, gramOf(lower, i))
			}
		}
	}
	walk(doc)

	if len(out) == 0 {
		return nil
	}

	slices.Sort(out)

	return slices.Compact(out)
}

func diffGrams(old, new []uint32) ([]uint32, []uint32) {
	var removed, added []uint32
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

func postingKey(gram uint32, chunk uint32) string {
	var b [3 + 4]byte
	b[0], b[1], b[2] = byte(gram>>16), byte(gram>>8), byte(gram)
	binary.BigEndian.PutUint32(b[3:], chunk)

	return string(b[:])
}

func postingAdd(c []byte, off uint16) []byte {
	if len(c) == 0 {
		return []byte{0, byte(off), byte(off >> 8)}
	}

	if c[0] == 1 {
		out := append([]byte(nil), c...)
		out[1+off/8] |= 1 << (off % 8)
		return out
	}

	n := (len(c) - 1) / 2
	i := sort.Search(n, func(i int) bool { return binary.LittleEndian.Uint16(c[1+2*i:]) >= off })
	if i < n && binary.LittleEndian.Uint16(c[1+2*i:]) == off {
		return c
	}

	if n+1 > postingArrayMax {
		out := make([]byte, 1+postingBitmap)
		out[0] = 1
		for j := 0; j < n; j++ {
			v := binary.LittleEndian.Uint16(c[1+2*j:])
			out[1+v/8] |= 1 << (v % 8)
		}
		out[1+off/8] |= 1 << (off % 8)
		return out
	}

	out := make([]byte, 0, len(c)+2)
	out = append(out, c[:1+2*i]...)
	out = binary.LittleEndian.AppendUint16(out, off)

	return append(out, c[1+2*i:]...)
}

func postingRemove(c []byte, off uint16) []byte {
	if len(c) == 0 {
		return nil
	}

	if c[0] == 1 {
		out := append([]byte(nil), c...)
		out[1+off/8] &^= 1 << (off % 8)
		for _, b := range out[1:] {
			if b != 0 {
				return out
			}
		}
		return nil
	}

	n := (len(c) - 1) / 2
	i := sort.Search(n, func(i int) bool { return binary.LittleEndian.Uint16(c[1+2*i:]) >= off })
	if i >= n || binary.LittleEndian.Uint16(c[1+2*i:]) != off {
		return c
	}

	if n == 1 {
		return nil
	}

	out := make([]byte, 0, len(c)-2)
	out = append(out, c[:1+2*i]...)

	return append(out, c[1+2*i+2:]...)
}

func postingEach(c []byte, base uint32, fn func(seq uint32)) {
	if len(c) == 0 {
		return
	}

	if c[0] == 1 {
		for i, b := range c[1:] {
			for b != 0 {
				t := bits.TrailingZeros8(b)
				fn(base + uint32(i*8+t))
				b &= b - 1
			}
		}
		return
	}

	for j := 1; j+1 < len(c); j += 2 {
		fn(base + uint32(binary.LittleEndian.Uint16(c[j:])))
	}
}

func updatePostings(tree pagetree.Tree, seq uint32, removed, added []uint32) pagetree.Tree {
	chunk := seq >> postingChunkBits
	off := uint16(seq & (postingChunk - 1))

	for _, g := range removed {
		key := postingKey(g, chunk)
		cur, ok := tree.Get(key)
		if !ok {
			continue
		}

		if postingRemove(cur, off) == nil {
			tree, _ = tree.Delete(key)
			continue
		}

		tree, _ = tree.Update(key, func(c []byte) []byte {
			if c[0] == 1 {
				c[1+off/8] &^= 1 << (off % 8)
				return c
			}
			return postingRemove(c, off)
		})
	}

	for _, g := range added {
		tree = tree.Upsert(postingKey(g, chunk), func(c []byte, found bool) []byte {
			if !found {
				return postingAdd(nil, off)
			}
			if c[0] == 1 {
				c[1+off/8] |= 1 << (off % 8)
				return c
			}
			return postingAdd(c, off)
		})
	}

	return tree
}

func gramPostings(tree pagetree.Tree, gram uint32) *Bitset {
	out := &Bitset{}
	lo := postingKey(gram, 0)
	prefix := lo[:3]
	tree.AscendFrom(lo, func(key string, val []byte) bool {
		if key[:3] != prefix {
			return false
		}
		out.addContainer(binary.BigEndian.Uint32([]byte(key[3:])), val)
		return true
	})

	return out
}

func gramChunks(tree pagetree.Tree, gram uint32) int {
	return tree.Rank(postingKey(gram, 1<<32-1)+"\xff") - tree.Rank(postingKey(gram, 0))
}

func (e *Engine) SearchCandidates(name string, pattern string) (*Bitset, bool) {
	patternGrams := PatternTrigrams(pattern)
	if len(patternGrams) == 0 {
		return nil, false
	}

	grams := make([]uint32, len(patternGrams))
	for i, g := range patternGrams {
		grams[i] = gramOf(g, 0)
	}

	ent := e.entity(name, false)
	if ent == nil {
		return &Bitset{}, true
	}

	snap := e.Snapshot(name)
	view, ok := snap.Index(searchSpec())
	if !ok {
		if snap.Len() > 0 && (!snap.HasIndex(searchSpec()) || snap.IndexDormant(searchSpec())) {
			e.EnsureIndex(name, searchSpec())
			snap = e.Snapshot(name)
			view, ok = snap.Index(searchSpec())
		}
		if !ok {
			return nil, false
		}
	}

	tree := view.idx.tree
	sort.Slice(grams, func(i, j int) bool { return gramChunks(tree, grams[i]) < gramChunks(tree, grams[j]) })

	var out *Bitset
	for _, g := range grams {
		set := gramPostings(tree, g)
		if out == nil {
			out = set
		} else {
			out.And(set)
		}
		if out.Count() == 0 {
			return out, true
		}
	}

	return out, true
}
