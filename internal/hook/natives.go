package hook

import (
	"sort"
	"strconv"
	"strings"
	"sync"

	"github.com/dop251/goja"
	"github.com/taymour/elysiandb/internal/docpath"
)

type aggregate struct {
	count int64
	sum   float64
}

type aggregates struct {
	mu    sync.Mutex
	sums  map[string]float64
	group map[string]map[string]*aggregate
}

func (r *result) aggs() *aggregates {
	r.aggOnce.Do(func() {
		r.agg = &aggregates{sums: map[string]float64{}, group: map[string]map[string]*aggregate{}}
	})

	return r.agg
}

func (rw *row) value(field string) (any, bool) {
	if i := rw.find(field); i >= 0 {
		if rw.nested != nil && rw.nested[i] != nil {
			return rw.nested[i], true
		}
		return rw.vals[i].Export(), true
	}

	head, rest, ok := strings.Cut(field, ".")
	if !ok {
		return nil, false
	}

	i := rw.find(head)
	if i < 0 || rw.nested == nil || rw.nested[i] == nil {
		return nil, false
	}

	values := docpath.ResolveValues(map[string]any{head: rw.nested[i]}, head+"."+rest)
	if len(values) != 1 {
		return nil, false
	}

	return values[0], true
}

func number(v any) (float64, bool) {
	switch n := v.(type) {
	case float64:
		return n, true
	case int64:
		return float64(n), true
	case int:
		return float64(n), true
	}

	return 0, false
}

func groupKey(v any) (string, bool) {
	switch t := v.(type) {
	case string:
		return t, true
	case bool:
		return strconv.FormatBool(t), true
	}

	if n, ok := number(v); ok {
		return strconv.FormatFloat(n, 'f', -1, 64), true
	}

	return "", false
}

func (r *result) sum(field string) float64 {
	a := r.aggs()
	a.mu.Lock()
	defer a.mu.Unlock()

	if s, ok := a.sums[field]; ok {
		return s
	}

	total := 0.0
	for i := range r.rows {
		if v, ok := r.rows[i].value(field); ok {
			if n, ok := number(v); ok {
				total += n
			}
		}
	}
	a.sums[field] = total

	return total
}

func (r *result) groupBy(field, sumField string) map[string]*aggregate {
	key := field + "\x00" + sumField
	a := r.aggs()
	a.mu.Lock()
	defer a.mu.Unlock()

	if g, ok := a.group[key]; ok {
		return g
	}

	g := map[string]*aggregate{}
	for i := range r.rows {
		v, ok := r.rows[i].value(field)
		if !ok {
			continue
		}

		k, ok := groupKey(v)
		if !ok {
			continue
		}

		agg := g[k]
		if agg == nil {
			agg = &aggregate{}
			g[k] = agg
		}
		agg.count++

		if sumField != "" {
			if sv, ok := r.rows[i].value(sumField); ok {
				if n, ok := number(sv); ok {
					agg.sum += n
				}
			}
		}
	}
	a.group[key] = g

	return g
}

type groupView struct {
	vm      *goja.Runtime
	g       map[string]*aggregate
	withSum bool
	over    map[string]goja.Value
	deleted map[string]bool
	added   []string
}

func groupObject(vm *goja.Runtime, g map[string]*aggregate, withSum bool) goja.Value {
	return vm.NewDynamicObject(&groupView{vm: vm, g: g, withSum: withSum})
}

func (v *groupView) Get(key string) goja.Value {
	if val, ok := v.over[key]; ok {
		return val
	}

	agg, ok := v.g[key]
	if !ok || v.deleted[key] {
		return nil
	}

	entry := v.vm.NewObject()
	_ = entry.Set("count", agg.count)
	if v.withSum {
		_ = entry.Set("sum", agg.sum)
	}

	if v.over == nil {
		v.over = map[string]goja.Value{}
	}
	v.over[key] = entry

	return entry
}

func (v *groupView) Set(key string, val goja.Value) bool {
	if _, ok := v.over[key]; !ok {
		if _, base := v.g[key]; !base {
			v.added = append(v.added, key)
		}
	}

	if v.over == nil {
		v.over = map[string]goja.Value{}
	}
	v.over[key] = val
	delete(v.deleted, key)

	return true
}

func (v *groupView) Has(key string) bool {
	if _, ok := v.over[key]; ok {
		return true
	}

	_, ok := v.g[key]

	return ok && !v.deleted[key]
}

func (v *groupView) Delete(key string) bool {
	delete(v.over, key)
	if _, ok := v.g[key]; ok {
		if v.deleted == nil {
			v.deleted = map[string]bool{}
		}
		v.deleted[key] = true
	}

	for i, k := range v.added {
		if k == key {
			v.added = append(v.added[:i], v.added[i+1:]...)
			break
		}
	}

	return true
}

func (v *groupView) Keys() []string {
	keys := make([]string, 0, len(v.g)+len(v.added))
	for k := range v.g {
		if !v.deleted[k] {
			keys = append(keys, k)
		}
	}
	sort.Strings(keys)

	return append(keys, v.added...)
}
