package ptree

import "sort"

const (
	maxEntries = 32
	minEntries = 8
)

type Tree[V any] struct {
	root *node[V]
}

type node[V any] struct {
	keys  []string
	vals  []V
	kids  []*node[V]
	sizes []int
	size  int
}

func (n *node[V]) leaf() bool {
	return n.kids == nil
}

func (n *node[V]) minKey() string {
	return n.keys[0]
}

func (n *node[V]) clone() *node[V] {
	c := &node[V]{size: n.size, keys: append([]string(nil), n.keys...)}
	if n.leaf() {
		c.vals = append([]V(nil), n.vals...)
		return c
	}

	c.kids = append([]*node[V](nil), n.kids...)
	c.sizes = append([]int(nil), n.sizes...)

	return c
}

func childIndex(keys []string, key string) int {
	i := sort.Search(len(keys), func(j int) bool { return keys[j] > key }) - 1
	if i < 0 {
		return 0
	}

	return i
}

func searchLeaf(keys []string, key string) (int, bool) {
	i := sort.SearchStrings(keys, key)
	return i, i < len(keys) && keys[i] == key
}

func insertAt[T any](s []T, i int, v T) []T {
	out := make([]T, len(s)+1)
	copy(out, s[:i])
	out[i] = v
	copy(out[i+1:], s[i:])

	return out
}

func removeAt[T any](s []T, i int) []T {
	out := make([]T, len(s)-1)
	copy(out, s[:i])
	copy(out[i:], s[i+1:])

	return out
}

func (t Tree[V]) Len() int {
	if t.root == nil {
		return 0
	}

	return t.root.size
}

func (t Tree[V]) Get(key string) (V, bool) {
	n := t.root
	for n != nil {
		if n.leaf() {
			if i, ok := searchLeaf(n.keys, key); ok {
				return n.vals[i], true
			}

			break
		}

		n = n.kids[childIndex(n.keys, key)]
	}

	var zero V

	return zero, false
}

func (t Tree[V]) Put(key string, v V) (Tree[V], bool) {
	if t.root == nil {
		return Tree[V]{root: &node[V]{keys: []string{key}, vals: []V{v}, size: 1}}, false
	}

	n, split, replaced := put(t.root, key, v)
	if split == nil {
		return Tree[V]{root: n}, replaced
	}

	root := &node[V]{
		keys:  []string{n.minKey(), split.minKey()},
		kids:  []*node[V]{n, split},
		sizes: []int{n.size, split.size},
		size:  n.size + split.size,
	}

	return Tree[V]{root: root}, replaced
}

func put[V any](n *node[V], key string, v V) (*node[V], *node[V], bool) {
	if n.leaf() {
		i, found := searchLeaf(n.keys, key)
		if found {
			c := n.clone()
			c.vals[i] = v
			return c, nil, true
		}

		c := &node[V]{keys: insertAt(n.keys, i, key), vals: insertAt(n.vals, i, v), size: n.size + 1}
		if len(c.keys) <= maxEntries {
			return c, nil, false
		}

		left, right := splitLeaf(c)

		return left, right, false
	}

	i := childIndex(n.keys, key)
	kid, split, replaced := put(n.kids[i], key, v)

	c := n.clone()
	c.kids[i] = kid
	c.sizes[i] = kid.size
	c.keys[i] = kid.minKey()
	if !replaced {
		c.size++
	}

	if split != nil {
		c.keys = insertAt(c.keys, i+1, split.minKey())
		c.kids = insertAt(c.kids, i+1, split)
		c.sizes = insertAt(c.sizes, i+1, split.size)
		c.sizes[i] = kid.size
	}

	if len(c.kids) <= maxEntries {
		return c, nil, replaced
	}

	left, right := splitInternal(c)

	return left, right, replaced
}

func splitLeaf[V any](n *node[V]) (*node[V], *node[V]) {
	mid := len(n.keys) / 2
	left := &node[V]{keys: append([]string(nil), n.keys[:mid]...), vals: append([]V(nil), n.vals[:mid]...), size: mid}
	right := &node[V]{keys: append([]string(nil), n.keys[mid:]...), vals: append([]V(nil), n.vals[mid:]...), size: len(n.keys) - mid}

	return left, right
}

func splitInternal[V any](n *node[V]) (*node[V], *node[V]) {
	mid := len(n.kids) / 2
	left := &node[V]{keys: append([]string(nil), n.keys[:mid]...), kids: append([]*node[V](nil), n.kids[:mid]...), sizes: append([]int(nil), n.sizes[:mid]...)}
	right := &node[V]{keys: append([]string(nil), n.keys[mid:]...), kids: append([]*node[V](nil), n.kids[mid:]...), sizes: append([]int(nil), n.sizes[mid:]...)}
	for _, s := range left.sizes {
		left.size += s
	}

	for _, s := range right.sizes {
		right.size += s
	}

	return left, right
}

func (t Tree[V]) Delete(key string) (Tree[V], bool) {
	if t.root == nil {
		return t, false
	}

	n, removed := del(t.root, key)
	if !removed {
		return t, false
	}

	if n == nil || n.size == 0 {
		return Tree[V]{}, true
	}

	for !n.leaf() && len(n.kids) == 1 {
		n = n.kids[0]
	}

	return Tree[V]{root: n}, true
}

func del[V any](n *node[V], key string) (*node[V], bool) {
	if n.leaf() {
		i, found := searchLeaf(n.keys, key)
		if !found {
			return n, false
		}

		if len(n.keys) == 1 {
			return nil, true
		}

		return &node[V]{keys: removeAt(n.keys, i), vals: removeAt(n.vals, i), size: n.size - 1}, true
	}

	i := childIndex(n.keys, key)
	kid, removed := del(n.kids[i], key)
	if !removed {
		return n, false
	}

	c := n.clone()
	c.size--

	if kid == nil {
		if len(c.kids) == 1 {
			return nil, true
		}

		c.keys = removeAt(c.keys, i)
		c.kids = removeAt(c.kids, i)
		c.sizes = removeAt(c.sizes, i)

		return c, true
	}

	c.kids[i] = kid
	c.sizes[i] = kid.size
	c.keys[i] = kid.minKey()

	if entries(kid) < minEntries {
		mergeWithNeighbour(c, i)
	}

	return c, true
}

func entries[V any](n *node[V]) int {
	return len(n.keys)
}

func mergeWithNeighbour[V any](c *node[V], i int) {
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

	left, right := c.kids[a], c.kids[b]
	if entries(left)+entries(right) > maxEntries {
		return
	}

	merged := &node[V]{keys: append(append([]string(nil), left.keys...), right.keys...), size: left.size + right.size}
	if left.leaf() {
		merged.vals = append(append([]V(nil), left.vals...), right.vals...)
	} else {
		merged.kids = append(append([]*node[V](nil), left.kids...), right.kids...)
		merged.sizes = append(append([]int(nil), left.sizes...), right.sizes...)
	}

	c.kids[a] = merged
	c.sizes[a] = merged.size
	c.keys[a] = merged.minKey()
	c.keys = removeAt(c.keys, b)
	c.kids = removeAt(c.kids, b)
	c.sizes = removeAt(c.sizes, b)
}

func (t Tree[V]) Rank(key string) int {
	rank := 0
	n := t.root
	for n != nil {
		if n.leaf() {
			return rank + sort.SearchStrings(n.keys, key)
		}

		i := childIndex(n.keys, key)
		for j := 0; j < i; j++ {
			rank += n.sizes[j]
		}

		n = n.kids[i]
	}

	return rank
}

func (t Tree[V]) At(index int) (string, V, bool) {
	var zero V
	if t.root == nil || index < 0 || index >= t.root.size {
		return "", zero, false
	}

	n := t.root
	for !n.leaf() {
		i := 0
		for index >= n.sizes[i] {
			index -= n.sizes[i]
			i++
		}

		n = n.kids[i]
	}

	return n.keys[index], n.vals[index], true
}

func (t Tree[V]) AscendAt(index int, fn func(key string, v V) bool) {
	if t.root == nil || index >= t.root.size {
		return
	}

	if index < 0 {
		index = 0
	}

	ascendAt(t.root, index, fn)
}

func ascendAt[V any](n *node[V], index int, fn func(string, V) bool) bool {
	if n.leaf() {
		for i := index; i < len(n.keys); i++ {
			if !fn(n.keys[i], n.vals[i]) {
				return false
			}
		}

		return true
	}

	i := 0
	for i < len(n.kids) && index >= n.sizes[i] {
		index -= n.sizes[i]
		i++
	}

	for ; i < len(n.kids); i++ {
		if !ascendAt(n.kids[i], index, fn) {
			return false
		}

		index = 0
	}

	return true
}

func (t Tree[V]) AscendFrom(key string, fn func(key string, v V) bool) {
	if t.root == nil {
		return
	}

	ascendFrom(t.root, key, fn)
}

func ascendFrom[V any](n *node[V], key string, fn func(string, V) bool) bool {
	if n.leaf() {
		for i := sort.SearchStrings(n.keys, key); i < len(n.keys); i++ {
			if !fn(n.keys[i], n.vals[i]) {
				return false
			}
		}

		return true
	}

	for i := childIndex(n.keys, key); i < len(n.kids); i++ {
		if !ascendFrom(n.kids[i], key, fn) {
			return false
		}
	}

	return true
}

func (t Tree[V]) DescendBefore(key string, fn func(key string, v V) bool) {
	if t.root == nil {
		return
	}

	descendBefore(t.root, key, fn)
}

func descendBefore[V any](n *node[V], key string, fn func(string, V) bool) bool {
	if n.leaf() {
		for i := sort.SearchStrings(n.keys, key) - 1; i >= 0; i-- {
			if !fn(n.keys[i], n.vals[i]) {
				return false
			}
		}

		return true
	}

	for i := childIndex(n.keys, key); i >= 0; i-- {
		if !descendBefore(n.kids[i], key, fn) {
			return false
		}
	}

	return true
}

func (t Tree[V]) DescendAt(index int, fn func(key string, v V) bool) {
	if t.root == nil || index < 0 {
		return
	}

	if index >= t.root.size {
		index = t.root.size - 1
	}

	descendAt(t.root, index, fn)
}

func descendAt[V any](n *node[V], index int, fn func(string, V) bool) bool {
	if n.leaf() {
		for i := index; i >= 0; i-- {
			if !fn(n.keys[i], n.vals[i]) {
				return false
			}
		}

		return true
	}

	i := 0
	for i < len(n.kids)-1 && index >= n.sizes[i] {
		index -= n.sizes[i]
		i++
	}

	for ; i >= 0; i-- {
		if !descendAt(n.kids[i], index, fn) {
			return false
		}

		if i > 0 {
			index = n.sizes[i-1] - 1
		}
	}

	return true
}

func FromSorted[V any](keys []string, vals []V) Tree[V] {
	if len(keys) == 0 {
		return Tree[V]{}
	}

	level := []*node[V]{}
	for start := 0; start < len(keys); start += maxEntries {
		end := min(start+maxEntries, len(keys))
		level = append(level, &node[V]{
			keys: append([]string(nil), keys[start:end]...),
			vals: append([]V(nil), vals[start:end]...),
			size: end - start,
		})
	}

	for len(level) > 1 {
		next := []*node[V]{}
		for start := 0; start < len(level); start += maxEntries {
			end := min(start+maxEntries, len(level))
			n := &node[V]{}
			for _, kid := range level[start:end] {
				n.keys = append(n.keys, kid.minKey())
				n.kids = append(n.kids, kid)
				n.sizes = append(n.sizes, kid.size)
				n.size += kid.size
			}

			next = append(next, n)
		}

		level = next
	}

	return Tree[V]{root: level[0]}
}
