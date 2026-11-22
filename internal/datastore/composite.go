package datastore

import (
	"encoding/binary"
	"strings"

	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/pagetree"
)

const irregularKey = "\x00irregular"

func CompositeSpec(eqField, sortField string) IndexSpec {
	return IndexSpec{Kind: IndexComposite, Field: eqField + "," + sortField}
}

func (s IndexSpec) CompositeFields() (string, string, bool) {
	if s.Kind != IndexComposite {
		return "", "", false
	}

	i := strings.IndexByte(s.Field, ',')
	if i <= 0 || i == len(s.Field)-1 {
		return "", "", false
	}

	return s.Field[:i], s.Field[i+1:], true
}

func CompositePrefix(value string) []byte {
	return AppendTerminated([]byte{KeyLower}, strings.ToLower(value))
}

func compositeKeys(spec IndexSpec, doc map[string]any) ([]string, bool) {
	eqField, sortField, ok := spec.CompositeFields()
	if !ok || doc == nil {
		return nil, false
	}

	values := docpath.ResolveValues(doc, eqField)
	if len(values) == 0 {
		return nil, false
	}

	sortKey := SortKey(doc, sortField)
	keys := make([][]byte, 0, len(values))
	irregular := false
	for _, v := range values {
		s, isString := v.(string)
		if !isString {
			irregular = true
			continue
		}
		keys = append(keys, append(CompositePrefix(s), sortKey...))
	}

	return uniqueKeys(keys), irregular
}

func irregularCount(tree pagetree.Tree) int64 {
	b, ok := tree.Get(irregularKey)
	if !ok {
		return 0
	}

	n, _ := binary.Varint(b)

	return n
}

func addIrregular(tree pagetree.Tree, delta int64) pagetree.Tree {
	n := irregularCount(tree) + delta
	if n == 0 {
		tree, _ = tree.Delete(irregularKey)
		return tree
	}

	tree, _ = tree.Put(irregularKey, binary.AppendVarint(nil, n))

	return tree
}

func applyCompositeChange(spec IndexSpec, tree pagetree.Tree, seq uint32, oldDoc, newDoc map[string]any) (pagetree.Tree, bool) {
	oldKeys, oldIrregular := compositeKeys(spec, oldDoc)
	newKeys, newIrregular := compositeKeys(spec, newDoc)
	if oldIrregular == newIrregular && equalStrings(oldKeys, newKeys) {
		return tree, false
	}

	removed, added := diffSorted(oldKeys, newKeys)
	sk := seqKey(seq)
	for _, k := range removed {
		tree, _ = tree.Delete(k + sk)
	}

	for _, k := range added {
		tree, _ = tree.Put(k+sk, nil)
	}

	switch {
	case oldIrregular && !newIrregular:
		tree = addIrregular(tree, -1)
	case !oldIrregular && newIrregular:
		tree = addIrregular(tree, 1)
	}

	return tree, true
}

func (v *IndexView) Irregular() int64 {
	return irregularCount(v.idx.tree)
}

func (v *IndexView) Sub(prefix []byte) *IndexView {
	p := string(prefix)
	base := v.idx.tree.Rank(p)
	end := v.idx.tree.Rank(string(prefixEnd(prefix)))

	return &IndexView{s: v.s, idx: v.idx, prefix: p, base: base, end: end, sub: true}
}

func (v *IndexView) subLen() int {
	return v.end - v.base
}

func (v *IndexView) subRank(key []byte) int {
	r := v.idx.tree.Rank(v.prefix+string(key)) - v.base

	return min(max(r, 0), v.subLen())
}

func (v *IndexView) subAt(i int) (string, uint32, bool) {
	if i < 0 || i >= v.subLen() {
		return "", 0, false
	}

	k, _, ok := v.idx.tree.At(v.base + i)
	if !ok {
		return "", 0, false
	}

	return k[len(v.prefix):], keySeq(k), true
}

func (v *IndexView) subAscendAt(i int, fn func(key string, seq uint32) bool) {
	if i < 0 || i >= v.subLen() {
		return
	}

	left := v.subLen() - i
	v.idx.tree.AscendAt(v.base+i, func(k string, _ []byte) bool {
		if left <= 0 {
			return false
		}
		left--
		return fn(k[len(v.prefix):], keySeq(k))
	})
}
