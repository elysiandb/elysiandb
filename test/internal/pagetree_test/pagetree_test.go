package pagetree_test

import (
	"bytes"
	"fmt"
	"math/rand"
	"sort"
	"sync"
	"testing"

	"github.com/taymour/elysiandb/internal/pagetree"
)

type memStore struct {
	mu    sync.Mutex
	pages map[uint64][]byte
	next  uint64
	refs  []*pagetree.Ref
	loads int
}

func newMemStore() *memStore {
	return &memStore{pages: map[uint64][]byte{}, next: 1}
}

func (s *memStore) Load(ref *pagetree.Ref) (*pagetree.Page, error) {
	s.mu.Lock()
	b, ok := s.pages[ref.ID()]
	s.loads++
	s.mu.Unlock()
	if !ok {
		return nil, fmt.Errorf("page %d not found", ref.ID())
	}

	p, err := pagetree.Decode(b)
	if err != nil {
		return nil, err
	}

	return ref.Attach(p), nil
}

func (s *memStore) flush(t pagetree.Tree) {
	err := t.ForEachDirty(func(ref *pagetree.Ref, p *pagetree.Page) error {
		b, err := pagetree.Encode(nil, p)
		if err != nil {
			return err
		}
		s.mu.Lock()
		id := s.next
		s.next++
		s.pages[id] = b
		s.refs = append(s.refs, ref)
		s.mu.Unlock()
		ref.Persisted(id, uint32(len(b)))
		return nil
	})
	if err != nil {
		panic(err)
	}
}

func (s *memStore) evict(r *rand.Rand, share float64) {
	for _, ref := range s.refs {
		if r.Float64() < share {
			ref.Evict()
		}
	}
}

type model map[string]string

func (m model) sorted() []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

func check(t *testing.T, step int, tr pagetree.Tree, m model) {
	t.Helper()

	keys := m.sorted()
	if tr.Len() != len(keys) {
		t.Fatalf("step %d: len %d, want %d", step, tr.Len(), len(keys))
	}

	i := 0
	tr.AscendAt(0, func(k string, v []byte) bool {
		if i >= len(keys) || k != keys[i] || string(v) != m[k] {
			t.Fatalf("step %d: ascend at %d got %q=%q, want %q=%q", step, i, k, v, keys[i], m[keys[i]])
		}
		i++
		return true
	})
	if i != len(keys) {
		t.Fatalf("step %d: ascend saw %d keys, want %d", step, i, len(keys))
	}

	for _, probe := range []int{0, 1, len(keys) / 3, len(keys) / 2, len(keys) - 1, len(keys)} {
		if probe < 0 {
			continue
		}
		k, v, ok := tr.At(probe)
		if probe < len(keys) && (!ok || k != keys[probe] || string(v) != m[k]) {
			t.Fatalf("step %d: At(%d) = %q %v, want %q", step, probe, k, ok, keys[probe])
		}
		if probe >= len(keys) && ok {
			t.Fatalf("step %d: At(%d) beyond the end", step, probe)
		}

		var asc []string
		tr.AscendAt(probe, func(k string, _ []byte) bool { asc = append(asc, k); return len(asc) < 5 })
		want := keys[min(probe, len(keys)):min(probe+5, len(keys))]
		if len(asc) != len(want) || (len(want) > 0 && asc[0] != want[0]) {
			t.Fatalf("step %d: AscendAt(%d) %v, want %v", step, probe, asc, want)
		}

		if probe < len(keys) {
			var desc []string
			tr.DescendAt(probe, func(k string, _ []byte) bool { desc = append(desc, k); return len(desc) < 5 })
			if len(desc) == 0 || desc[0] != keys[probe] {
				t.Fatalf("step %d: DescendAt(%d) %v", step, probe, desc)
			}
			for j := 1; j < len(desc); j++ {
				if desc[j] != keys[probe-j] {
					t.Fatalf("step %d: DescendAt(%d) %v", step, probe, desc)
				}
			}
		}
	}

	for _, k := range []string{"", "k0", "k5000", "k99", "z", keys0(keys)} {
		rank := sort.SearchStrings(keys, k)
		if got := tr.Rank(k); got != rank {
			t.Fatalf("step %d: Rank(%q) %d, want %d", step, k, got, rank)
		}

		var from []string
		tr.AscendFrom(k, func(key string, _ []byte) bool { from = append(from, key); return len(from) < 3 })
		want := keys[rank:min(rank+3, len(keys))]
		if fmt.Sprint(from) != fmt.Sprint(want) && !(len(from) == 0 && len(want) == 0) {
			t.Fatalf("step %d: AscendFrom(%q) %v, want %v", step, k, from, want)
		}

		var before []string
		tr.DescendBefore(k, func(key string, _ []byte) bool { before = append(before, key); return len(before) < 3 })
		for j, key := range before {
			if key != keys[rank-1-j] {
				t.Fatalf("step %d: DescendBefore(%q) %v", step, k, before)
			}
		}

		v, ok := tr.Get(k)
		mv, mok := m[k]
		if ok != mok || (ok && string(v) != mv) {
			t.Fatalf("step %d: Get(%q) %q %v, want %q %v", step, k, v, ok, mv, mok)
		}
	}
}

func keys0(keys []string) string {
	if len(keys) == 0 {
		return "k1"
	}
	return keys[len(keys)/2]
}

func TestRandomOperationsAgainstASortedMap(t *testing.T) {
	r := rand.New(rand.NewSource(1))
	store := newMemStore()
	tr := pagetree.New(store)
	m := model{}

	type version struct {
		tree pagetree.Tree
		data model
	}
	var versions []version

	for step := 0; step < 400; step++ {
		tok := pagetree.NewToken()
		w := tr.WithToken(tok)
		for op := 0; op < 1+r.Intn(200); op++ {
			k := fmt.Sprintf("k%d", r.Intn(5000))
			switch {
			case r.Intn(6) == 0:
				grow := r.Intn(2) == 0
				w, _ = w.Update(k, func(v []byte) []byte {
					if grow {
						return append(append([]byte(nil), v...), 'g')
					}
					if len(v) > 0 {
						v[0] = 'Z'
					}
					return v
				})
				if v, ok := m[k]; ok {
					if grow {
						m[k] = v + "g"
					} else if len(v) > 0 {
						m[k] = "Z" + v[1:]
					}
				}
			case r.Intn(5) == 0:
				w = w.Upsert(k, func(v []byte, found bool) []byte {
					if !found {
						return []byte("new")
					}
					return append(append([]byte(nil), v...), 'u')
				})
				if v, ok := m[k]; ok {
					m[k] = v + "u"
				} else {
					m[k] = "new"
				}
			case r.Intn(3) == 0:
				w, _ = w.Delete(k)
				delete(m, k)
			default:
				v := bytes.Repeat([]byte{byte('a' + r.Intn(26))}, r.Intn(40))
				w, _ = w.Put(k, v)
				m[k] = string(v)
			}
		}
		tok.Freeze()
		tr = w.WithToken(nil)

		if step%7 == 0 {
			copied := model{}
			for k, v := range m {
				copied[k] = v
			}
			versions = append(versions, version{tree: tr, data: copied})
		}

		if step%5 == 0 {
			store.flush(tr)
			store.evict(r, 0.6)
		}

		check(t, step, tr, m)
	}

	for i, v := range versions {
		check(t, 10000+i, v.tree, v.data)
	}

	if store.loads == 0 {
		t.Fatalf("evicted pages should have been loaded back")
	}
}

func TestFromSortedMatchesInserts(t *testing.T) {
	store := newMemStore()
	m := model{}
	keys := []string{}
	vals := [][]byte{}
	for i := 0; i < 20000; i++ {
		k := fmt.Sprintf("key%07d", i*3)
		keys = append(keys, k)
		vals = append(vals, []byte(fmt.Sprint(i)))
		m[k] = fmt.Sprint(i)
	}

	tr := pagetree.FromSorted(store, keys, vals)
	check(t, 0, tr, m)

	store.flush(tr)
	store.evict(rand.New(rand.NewSource(2)), 1)
	check(t, 1, tr, m)

	tok := pagetree.NewToken()
	w := tr.WithToken(tok)
	for i := 0; i < 5000; i++ {
		k := fmt.Sprintf("key%07d", i*7+1)
		w, _ = w.Put(k, []byte("x"))
		m[k] = "x"
	}
	tok.Freeze()
	check(t, 2, w, m)
}

func TestAPublishedTreeNeverChanges(t *testing.T) {
	tok := pagetree.NewToken()
	tr := pagetree.New(nil).WithToken(tok)
	for i := 0; i < 3000; i++ {
		tr, _ = tr.Put(fmt.Sprintf("a%05d", i), []byte("v1"))
	}
	tok.Freeze()
	published := tr.WithToken(nil)

	next := published.WithToken(pagetree.NewToken())
	for i := 0; i < 3000; i += 2 {
		next, _ = next.Put(fmt.Sprintf("a%05d", i), []byte("v2"))
		next, _ = next.Delete(fmt.Sprintf("a%05d", i+1))
	}

	if published.Len() != 3000 {
		t.Fatalf("a later batch changed a published tree: len %d", published.Len())
	}
	published.AscendAt(0, func(k string, v []byte) bool {
		if string(v) != "v1" {
			t.Fatalf("a later batch changed %s to %s", k, v)
		}
		return true
	})
	if next.Len() != 1500 {
		t.Fatalf("the new version has %d keys, want 1500", next.Len())
	}

	stale := published.WithToken(tok)
	stale, _ = stale.Put("a00000", []byte("v3"))
	if v, _ := published.Get("a00000"); string(v) != "v1" {
		t.Fatalf("a frozen token must not change a published page, got %s", v)
	}
}

func TestEncodeDecodeRoundTrip(t *testing.T) {
	store := newMemStore()
	tr := pagetree.New(store).WithToken(pagetree.NewToken())
	for i := 0; i < 5000; i++ {
		tr, _ = tr.Put(fmt.Sprintf("%08d\x00\xff", i), []byte{byte(i), 0, 1})
	}
	store.flush(tr)

	for id, b := range store.pages {
		p, err := pagetree.Decode(b)
		if err != nil {
			t.Fatalf("page %d: %v", id, err)
		}
		again, err := pagetree.Encode(nil, p)
		if err != nil || !bytes.Equal(again, b) {
			t.Fatalf("page %d does not round trip", id)
		}
	}

	for _, broken := range [][]byte{nil, {9}, {1, 200}, {1, 1, 5, 'a'}} {
		if _, err := pagetree.Decode(broken); err == nil {
			t.Fatalf("a corrupt page %v is refused", broken)
		}
	}
}

func TestRelocateMovesOnlyVictimPages(t *testing.T) {
	store := newMemStore()
	m := model{}
	tr := pagetree.New(store).WithToken(pagetree.NewToken())
	for i := 0; i < 20000; i++ {
		k := fmt.Sprintf("r%06d", i)
		tr, _ = tr.Put(k, []byte(k))
		m[k] = k
	}
	tr = tr.WithToken(nil)
	store.flush(tr)
	store.evict(rand.New(rand.NewSource(3)), 1)

	var stored []uint64
	tr.WalkStored(func(id uint64, size uint32) { stored = append(stored, id) })
	if len(stored) != len(store.pages) {
		t.Fatalf("the walk sees %d stored pages, %d were written", len(stored), len(store.pages))
	}

	victims := map[uint64]bool{}
	for _, id := range stored[:len(stored)/3] {
		victims[id] = true
	}

	loadsBefore := store.loads
	moved, n := tr.WithToken(pagetree.NewToken()).Relocate(func(id uint64) bool { return victims[id] }, 1<<30)
	if n != len(victims) {
		t.Fatalf("moved %d pages, want %d", n, len(victims))
	}
	if store.loads-loadsBefore > len(victims)+len(stored)/40+10 {
		t.Fatalf("relocation loaded %d pages for %d victims", store.loads-loadsBefore, len(victims))
	}

	moved = moved.WithToken(nil)
	moved.WalkStored(func(id uint64, size uint32) {
		if victims[id] {
			t.Fatalf("page %d is still referenced after the relocation", id)
		}
	})
	check(t, 0, moved, m)
	check(t, 1, tr, m)
}
