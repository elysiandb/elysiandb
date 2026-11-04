package datastore

import (
	"encoding/binary"
	"io"
	"math/bits"
	"slices"
	"strings"

	"github.com/cockroachdb/pebble/v2"
	"github.com/taymour/elysiandb/internal/docpath"
)

const (
	postingChunkBits = 12
	postingChunk     = 1 << postingChunkBits
	postingArrayMax  = 256
	postingBitmap    = postingChunk / 8
	postingWords     = postingChunk / 64
)

const (
	postingArray     byte = 0
	postingFull      byte = 1
	postingDeltaList byte = 2
	postingDeltaBits byte = 3
	mergerName            = "elysiandb.postings.v1"
)

type chunkBits [postingWords]uint64

func (c *chunkBits) set(off uint16) {
	c[off/64] |= 1 << (off % 64)
}

func (c *chunkBits) clear(off uint16) {
	c[off/64] &^= 1 << (off % 64)
}

func (c *chunkBits) empty() bool {
	for _, w := range c {
		if w != 0 {
			return false
		}
	}

	return true
}

func (c *chunkBits) count() int {
	n := 0
	for _, w := range c {
		n += bits.OnesCount64(w)
	}

	return n
}

func (c *chunkBits) andNot(o *chunkBits) {
	for i := range c {
		c[i] &^= o[i]
	}
}

func (c *chunkBits) or(o *chunkBits) {
	for i := range c {
		c[i] |= o[i]
	}
}

func (c *chunkBits) each(fn func(off uint16)) {
	for wi, w := range c {
		for w != 0 {
			t := bits.TrailingZeros64(w)
			fn(uint16(wi*64 + t))
			w &= w - 1
		}
	}
}

func (c *chunkBits) appendTo(dst []byte) []byte {
	for _, w := range c {
		dst = binary.LittleEndian.AppendUint64(dst, w)
	}

	return dst
}

func (c *chunkBits) readFrom(b []byte) bool {
	if len(b) < postingBitmap {
		return false
	}

	for i := range c {
		c[i] = binary.LittleEndian.Uint64(b[8*i:])
	}

	return true
}

func encodeFullContainer(c *chunkBits) []byte {
	n := c.count()
	if n > postingArrayMax {
		return c.appendTo([]byte{postingFull})
	}

	out := make([]byte, 1, 1+2*n)
	out[0] = postingArray
	c.each(func(off uint16) {
		out = binary.LittleEndian.AppendUint16(out, off)
	})

	return out
}

func encodeDelta(adds, removes *chunkBits) []byte {
	na, nr := adds.count(), removes.count()
	if na+nr > postingArrayMax {
		out := adds.appendTo([]byte{postingDeltaBits})
		return removes.appendTo(out)
	}

	out := make([]byte, 0, 1+binary.MaxVarintLen16+2*(na+nr))
	out = append(out, postingDeltaList)
	out = binary.AppendUvarint(out, uint64(na))
	adds.each(func(off uint16) {
		out = binary.LittleEndian.AppendUint16(out, off)
	})
	removes.each(func(off uint16) {
		out = binary.LittleEndian.AppendUint16(out, off)
	})

	return out
}

func postingAddOp(off uint16) []byte {
	return []byte{postingDeltaList, 1, byte(off), byte(off >> 8)}
}

func postingRemoveOp(off uint16) []byte {
	return []byte{postingDeltaList, 0, byte(off), byte(off >> 8)}
}

type postingOperand struct {
	full    bool
	base    chunkBits
	adds    chunkBits
	removes chunkBits
}

func decodePosting(b []byte, op *postingOperand) bool {
	*op = postingOperand{}
	if len(b) == 0 {
		return false
	}

	switch b[0] {
	case postingArray:
		op.full = true
		if (len(b)-1)%2 != 0 {
			return false
		}
		for i := 1; i+1 < len(b); i += 2 {
			op.base.set(binary.LittleEndian.Uint16(b[i:]) & (postingChunk - 1))
		}
		return true
	case postingFull:
		op.full = true
		return op.base.readFrom(b[1:])
	case postingDeltaList:
		na, n := binary.Uvarint(b[1:])
		if n <= 0 || (len(b)-1-n)%2 != 0 {
			return false
		}
		rest := b[1+n:]
		if uint64(len(rest)/2) < na {
			return false
		}
		for i := 0; i+1 < len(rest); i += 2 {
			off := binary.LittleEndian.Uint16(rest[i:]) & (postingChunk - 1)
			if uint64(i/2) < na {
				op.adds.set(off)
			} else {
				op.removes.set(off)
			}
		}
		return true
	case postingDeltaBits:
		return len(b) == 1+2*postingBitmap && op.adds.readFrom(b[1:]) && op.removes.readFrom(b[1+postingBitmap:])
	}

	return false
}

type postingState struct {
	hasBase bool
	base    chunkBits
	adds    chunkBits
	removes chunkBits
}

func (s *postingState) apply(op *postingOperand) {
	if op.full {
		s.hasBase = true
		s.base = op.base
		s.adds = chunkBits{}
		s.removes = chunkBits{}
		return
	}

	if s.hasBase {
		s.base.andNot(&op.removes)
		s.base.or(&op.adds)
		return
	}

	s.adds.andNot(&op.removes)
	s.adds.or(&op.adds)
	s.removes.andNot(&op.adds)
	s.removes.or(&op.removes)
}

type postingMerger struct {
	older [][]byte
	newer [][]byte
}

func newPostingMerger(_, value []byte) (pebble.ValueMerger, error) {
	return &postingMerger{newer: [][]byte{slices.Clone(value)}}, nil
}

func (m *postingMerger) MergeNewer(value []byte) error {
	m.newer = append(m.newer, slices.Clone(value))
	return nil
}

func (m *postingMerger) MergeOlder(value []byte) error {
	m.older = append(m.older, slices.Clone(value))
	return nil
}

func (m *postingMerger) Finish(includesBase bool) ([]byte, io.Closer, error) {
	var st postingState
	var op postingOperand
	visit := func(b []byte) error {
		if !decodePosting(b, &op) {
			return pebble.ErrCorruption
		}
		st.apply(&op)
		return nil
	}

	for i := len(m.older) - 1; i >= 0; i-- {
		if err := visit(m.older[i]); err != nil {
			return nil, nil, err
		}
	}

	for _, b := range m.newer {
		if err := visit(b); err != nil {
			return nil, nil, err
		}
	}

	switch {
	case st.hasBase:
		return encodeFullContainer(&st.base), nil, nil
	case includesBase:
		return encodeFullContainer(&st.adds), nil, nil
	}

	return encodeDelta(&st.adds, &st.removes), nil, nil
}

var postingsMerger = &pebble.Merger{Name: mergerName, Merge: newPostingMerger}

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

func postingEntry(gram uint32, chunk uint32) []byte {
	var b [3 + 4]byte
	b[0], b[1], b[2] = byte(gram>>16), byte(gram>>8), byte(gram)
	binary.BigEndian.PutUint32(b[3:], chunk)

	return b[:]
}

func (b *Bitset) addContainer(chunk uint32, c []byte) {
	var op postingOperand
	if !decodePosting(c, &op) || !op.full {
		return
	}

	first := int(chunk) * postingWords
	if need := first + postingWords; need > len(b.words) {
		grown := make([]uint64, need+len(b.words)/2)
		copy(grown, b.words)
		b.words = grown
	}

	for w := 0; w < postingWords; w++ {
		b.words[first+w] |= op.base[w]
	}
}
