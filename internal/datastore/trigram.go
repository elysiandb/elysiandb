package datastore

import (
	"math/bits"
	"strings"
	"sync"

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

type trigramIndex struct {
	mu       sync.RWMutex
	postings map[string]*Bitset
	ready    bool
	updates  int
	docs     int
}

func newTrigramIndex() *trigramIndex {
	return &trigramIndex{postings: map[string]*Bitset{}}
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

func (t *trigramIndex) add(seq uint32, doc map[string]any) {
	grams := DocumentTrigrams(doc)
	t.mu.Lock()
	t.updates++
	for g := range grams {
		b := t.postings[g]
		if b == nil {
			b = &Bitset{}
			t.postings[g] = b
		}
		b.Set(seq)
	}
	t.mu.Unlock()
}

func (t *trigramIndex) candidates(grams []string) (*Bitset, bool) {
	t.mu.RLock()
	defer t.mu.RUnlock()

	if !t.ready {
		return nil, false
	}

	var out *Bitset
	for _, g := range grams {
		b := t.postings[g]
		if b == nil {
			return &Bitset{}, true
		}

		if out == nil {
			out = b.Clone()
		} else {
			out.And(b)
		}
	}

	if out == nil {
		return nil, false
	}

	return out, true
}

func (e *Engine) SearchCandidates(name string, pattern string) (*Bitset, bool) {
	grams := PatternTrigrams(pattern)
	if len(grams) == 0 {
		return nil, false
	}

	ent := e.entity(name, false)
	if ent == nil {
		return &Bitset{}, true
	}

	t := ent.search.Load()
	if t == nil {
		e.startSearchIndex(ent)
		return nil, false
	}

	t.mu.RLock()
	stale := t.ready && t.updates > 2*t.docs+1024
	t.mu.RUnlock()
	if stale {
		e.startSearchIndex(ent)
	}

	return t.candidates(grams)
}

func (e *Engine) startSearchIndex(ent *entity) {
	t := newTrigramIndex()

	ent.apply.Lock()
	current := ent.search.Load()
	if current != nil && !current.ready {
		ent.apply.Unlock()
		return
	}
	ent.search.Store(t)
	snap := ent.root.Load()
	ent.apply.Unlock()

	run := func() {
		docs := 0
		snap.slots.AscendAt(0, func(_ string, s *slot) bool {
			if doc := e.decodeSlot(s); doc != nil {
				t.add(s.seq, doc)
			}
			docs++
			return true
		})

		t.mu.Lock()
		t.ready = true
		t.docs = docs
		t.updates = docs
		t.mu.Unlock()
	}

	if e.opts.Background && !e.closed.Load() {
		e.wg.Add(1)
		go func() {
			defer e.wg.Done()
			run()
		}()
		return
	}

	run()
}
