package datastore

import (
	"math/bits"
	"strings"

	"github.com/taymour/elysiandb/internal/docpath"
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
