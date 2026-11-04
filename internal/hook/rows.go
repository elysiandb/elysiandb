package hook

import (
	"sort"
	"strings"

	"github.com/dop251/goja"
	"github.com/taymour/elysiandb/internal/storage"
)

type row struct {
	keys   []string
	vals   []goja.Value
	nested []any
	seq    uint32
	size   uint32
	lsn    uint64
}

func (r *row) find(key string) int {
	for i, k := range r.keys {
		if len(k) == len(key) && k == key {
			return i
		}
	}

	return -1
}

const (
	internedMaxLen   = 32
	internedMaxCount = 1 << 16
	rowOverhead      = 48
	fieldOverhead    = 24
	nestedFactor     = 4
)

type rowBuilder struct {
	rt      *goja.Runtime
	shapes  map[string][]string
	strings map[string]goja.Value
	bytes   int64
}

func newRowBuilder() *rowBuilder {
	return &rowBuilder{rt: goja.New(), shapes: map[string][]string{}, strings: map[string]goja.Value{}}
}

func (b *rowBuilder) shape(doc map[string]any) []string {
	keys := make([]string, 0, len(doc))
	for k := range doc {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	sig := strings.Join(keys, "\x00")
	if shared, ok := b.shapes[sig]; ok {
		return shared
	}

	b.shapes[sig] = keys
	b.bytes += int64(len(sig)) + int64(len(keys))*16

	return keys
}

func (b *rowBuilder) str(s string) goja.Value {
	if len(s) <= internedMaxLen {
		if v, ok := b.strings[s]; ok {
			return v
		}
	}

	v := b.rt.ToValue(s)
	b.bytes += int64(len(s)) + 16
	if len(s) <= internedMaxLen && len(b.strings) < internedMaxCount {
		b.strings[s] = v
	}

	return v
}

func (b *rowBuilder) build(doc map[string]any, seq uint32, lsn uint64, size int) row {
	before := b.bytes
	keys := b.shape(doc)
	r := row{keys: keys, vals: make([]goja.Value, len(keys)), seq: seq, lsn: lsn}
	b.bytes += rowOverhead + int64(len(keys))*fieldOverhead
	defer func() { r.size = uint32(min(b.bytes-before, 1<<31)) }()

	for i, k := range keys {
		switch v := doc[k].(type) {
		case nil:
			r.vals[i] = goja.Null()
		case string:
			r.vals[i] = b.str(v)
		case float64, bool:
			r.vals[i] = b.rt.ToValue(v)
		default:
			if r.nested == nil {
				r.nested = make([]any, len(keys))
				b.bytes += int64(len(keys)) * 16
			}
			r.nested[i] = v
		}
	}

	if r.nested != nil {
		b.bytes += int64(size) * nestedFactor
	}

	return r
}

type rowView struct {
	vm      *goja.Runtime
	r       *row
	over    map[string]goja.Value
	deleted map[string]bool
	added   []string
}

func (v *rowView) base(key string) (goja.Value, bool) {
	i := v.r.find(key)
	if i < 0 {
		return nil, false
	}

	if v.r.nested != nil && v.r.nested[i] != nil {
		copied := deepCopyAny(v.r.nested[i])
		val := v.vm.ToValue(copied)
		if v.over == nil {
			v.over = map[string]goja.Value{}
		}
		v.over[key] = val
		return val, true
	}

	return v.r.vals[i], true
}

func deepCopyAny(value any) any {
	switch typed := value.(type) {
	case map[string]any:
		return storage.DeepCopyDocument(typed)
	case []any:
		copied := make([]any, len(typed))
		for i, item := range typed {
			copied[i] = deepCopyAny(item)
		}
		return copied
	}

	return value
}

func (v *rowView) Get(key string) goja.Value {
	if val, ok := v.over[key]; ok {
		return val
	}

	if v.deleted[key] {
		return nil
	}

	val, _ := v.base(key)

	return val
}

func (v *rowView) Set(key string, val goja.Value) bool {
	if v.over == nil {
		v.over = map[string]goja.Value{}
	}

	if _, ok := v.over[key]; !ok && v.r.find(key) < 0 {
		v.added = append(v.added, key)
	}

	delete(v.deleted, key)
	v.over[key] = val

	return true
}

func (v *rowView) Has(key string) bool {
	if _, ok := v.over[key]; ok {
		return true
	}

	return !v.deleted[key] && v.r.find(key) >= 0
}

func (v *rowView) Delete(key string) bool {
	_, set := v.over[key]
	delete(v.over, key)

	if v.r.find(key) >= 0 {
		if v.deleted == nil {
			v.deleted = map[string]bool{}
		}
		v.deleted[key] = true
	}

	if set {
		for i, k := range v.added {
			if k == key {
				v.added = append(v.added[:i], v.added[i+1:]...)
				break
			}
		}
	}

	return true
}

func (v *rowView) Keys() []string {
	out := make([]string, 0, len(v.r.keys)+len(v.added))
	for _, k := range v.r.keys {
		if !v.deleted[k] {
			out = append(out, k)
		}
	}

	return append(out, v.added...)
}

type arrayView struct {
	vm     *goja.Runtime
	rows   []row
	items  []goja.Value
	base   int
	length int
}

func newArrayView(vm *goja.Runtime, rows []row) goja.Value {
	return vm.NewDynamicArray(&arrayView{vm: vm, rows: rows, base: len(rows), length: len(rows)})
}

func (a *arrayView) Len() int {
	return a.length
}

func (a *arrayView) grow(n int) {
	if len(a.items) >= n {
		return
	}

	if cap(a.items) >= n {
		a.items = a.items[:n]
		return
	}

	items := make([]goja.Value, n, max(n, 2*cap(a.items)))
	copy(items, a.items)
	a.items = items
}

func (a *arrayView) Get(i int) goja.Value {
	if i < 0 || i >= a.length {
		return nil
	}

	if i < len(a.items) && a.items[i] != nil {
		return a.items[i]
	}

	if i >= a.base {
		return nil
	}

	v := a.vm.NewDynamicObject(&rowView{vm: a.vm, r: &a.rows[i]})
	a.grow(a.length)
	a.items[i] = v

	return v
}

func (a *arrayView) Set(i int, val goja.Value) bool {
	if i < 0 {
		return false
	}

	if i >= a.length {
		a.length = i + 1
	}

	a.grow(a.length)
	a.items[i] = val

	return true
}

func (a *arrayView) SetLen(n int) bool {
	if n < 0 {
		return false
	}

	if n < a.base {
		a.base = n
	}

	if n < len(a.items) {
		clear(a.items[n:])
		a.items = a.items[:n]
	}

	a.length = n

	return true
}
