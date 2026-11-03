package ptree_test

import (
	"fmt"
	"math/rand"
	"sort"
	"testing"

	"github.com/taymour/elysiandb/internal/ptree"
)

type model map[string]int

func (m model) sorted() []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	return keys
}

func check(t *testing.T, tree ptree.Tree[int], m model) {
	t.Helper()

	keys := m.sorted()
	if tree.Len() != len(keys) {
		t.Fatalf("len %d, want %d", tree.Len(), len(keys))
	}

	i := 0
	tree.AscendAt(0, func(k string, v int) bool {
		if k != keys[i] || v != m[k] {
			t.Fatalf("ascend #%d: %s=%d, want %s=%d", i, k, v, keys[i], m[keys[i]])
		}
		i++
		return true
	})
	if i != len(keys) {
		t.Fatalf("ascend visited %d, want %d", i, len(keys))
	}

	for idx, k := range keys {
		if got, _, ok := tree.At(idx); !ok || got != k {
			t.Fatalf("At(%d)=%s, want %s", idx, got, k)
		}
		if r := tree.Rank(k); r != idx {
			t.Fatalf("Rank(%s)=%d, want %d", k, r, idx)
		}
		if v, ok := tree.Get(k); !ok || v != m[k] {
			t.Fatalf("Get(%s)=%d,%v", k, v, ok)
		}
	}

	j := len(keys) - 1
	tree.DescendAt(len(keys)-1, func(k string, _ int) bool {
		if k != keys[j] {
			t.Fatalf("descend #%d: %s, want %s", j, k, keys[j])
		}
		j--
		return true
	})
	if j != -1 {
		t.Fatalf("descend stopped at %d", j)
	}
}

func TestRandomOperationsAgainstModel(t *testing.T) {
	r := rand.New(rand.NewSource(7))
	tree := ptree.Tree[int]{}
	m := model{}
	snapshots := []struct {
		tree ptree.Tree[int]
		m    model
	}{}

	for step := 0; step < 20000; step++ {
		k := fmt.Sprintf("k%05d", r.Intn(3000))
		switch r.Intn(10) {
		case 0, 1, 2:
			var removed bool
			tree, removed = tree.Delete(k)
			_, existed := m[k]
			if removed != existed {
				t.Fatalf("delete %s removed=%v existed=%v", k, removed, existed)
			}
			delete(m, k)
		default:
			var replaced bool
			tree, replaced = tree.Put(k, step)
			_, existed := m[k]
			if replaced != existed {
				t.Fatalf("put %s replaced=%v existed=%v", k, replaced, existed)
			}
			m[k] = step
		}

		if step%2000 == 0 {
			cp := model{}
			for k, v := range m {
				cp[k] = v
			}
			snapshots = append(snapshots, struct {
				tree ptree.Tree[int]
				m    model
			}{tree, cp})
			check(t, tree, m)
		}
	}

	check(t, tree, m)
	for _, s := range snapshots {
		check(t, s.tree, s.m)
	}
}

func TestRangesAndBulkLoad(t *testing.T) {
	keys := []string{}
	vals := []int{}
	for i := 0; i < 5000; i++ {
		keys = append(keys, fmt.Sprintf("%06d", i*2))
		vals = append(vals, i)
	}

	tree := ptree.FromSorted(keys, vals)
	m := model{}
	for i, k := range keys {
		m[k] = vals[i]
	}
	check(t, tree, m)

	got := []string{}
	tree.AscendFrom("000101", func(k string, _ int) bool {
		got = append(got, k)
		return len(got) < 3
	})
	if fmt.Sprint(got) != "[000102 000104 000106]" {
		t.Fatalf("AscendFrom: %v", got)
	}

	got = got[:0]
	tree.DescendBefore("000101", func(k string, _ int) bool {
		got = append(got, k)
		return len(got) < 3
	})
	if fmt.Sprint(got) != "[000100 000098 000096]" {
		t.Fatalf("DescendBefore: %v", got)
	}

	if r := tree.Rank("000101"); r != 51 {
		t.Fatalf("Rank: %d", r)
	}

	for i := 0; i < 5000; i += 3 {
		tree, _ = tree.Delete(keys[i])
		delete(m, keys[i])
	}
	check(t, tree, m)
}
