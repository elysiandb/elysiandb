package hook

import (
	"bytes"
	"container/list"
	"crypto/sha256"
	"encoding/json"
	"math/rand"
	"sync"
	"sync/atomic"
	"time"

	"github.com/dop251/goja"
	"github.com/taymour/elysiandb/internal/memory"
	"github.com/taymour/elysiandb/internal/storage"
)

type queryDep struct {
	target  string
	filters map[string]map[string]string
	owner   string
	key     string
	version string
	bypass  bool
}

type output struct {
	deps    []queryDep
	set     map[string]any
	deleted []string
	bytes   int64
}

type outputItem struct {
	key  string
	out  []*output
	el   *list.Element
	used int64
}

type outputFlight struct {
	done chan struct{}
}

type outputMemo struct {
	mu      sync.Mutex
	items   map[string]*outputItem
	lru     *list.List
	flights map[string]*outputFlight
	hits    atomic.Int64
	misses  atomic.Int64
}

const maxOutputsPerInput = 8

var outputs = &outputMemo{items: map[string]*outputItem{}, lru: list.New(), flights: map[string]*outputFlight{}}

func memoKey(script, fnName string, input []byte) string {
	h := sha256.New()
	h.Write([]byte(script))
	h.Write([]byte{0})
	h.Write([]byte(fnName))
	h.Write([]byte{0})
	h.Write(input)

	return string(h.Sum(nil))
}

func canonical(entity map[string]any) ([]byte, bool) {
	if !jsonSafe(entity) {
		return nil, false
	}

	b, err := json.Marshal(entity)
	if err != nil {
		return nil, false
	}

	return b, true
}

func jsonSafe(v any) bool {
	switch t := v.(type) {
	case nil, bool, string, float64, int64:
		return true
	case map[string]any:
		for _, x := range t {
			if !jsonSafe(x) {
				return false
			}
		}
		return true
	case []any:
		for _, x := range t {
			if !jsonSafe(x) {
				return false
			}
		}
		return true
	}

	return false
}

func (m *outputMemo) find(key string, valid func(*output) bool) *output {
	m.mu.Lock()
	it, ok := m.items[key]
	var candidates []*output
	if ok {
		candidates = append(candidates, it.out...)
		m.lru.MoveToFront(it.el)
		it.used = time.Now().Unix()
	}
	m.mu.Unlock()

	for _, o := range candidates {
		if valid(o) {
			m.hits.Add(1)
			return o
		}
	}

	m.misses.Add(1)

	return nil
}

func (m *outputMemo) begin(key string) (func(), bool) {
	m.mu.Lock()
	if f, ok := m.flights[key]; ok {
		m.mu.Unlock()
		<-f.done
		return nil, false
	}

	f := &outputFlight{done: make(chan struct{})}
	m.flights[key] = f
	m.mu.Unlock()

	return func() {
		m.mu.Lock()
		delete(m.flights, key)
		m.mu.Unlock()
		close(f.done)
	}, true
}

func (m *outputMemo) store(key string, o *output) {
	m.mu.Lock()
	defer m.mu.Unlock()

	it, ok := m.items[key]
	if !ok {
		it = &outputItem{key: key}
		it.el = m.lru.PushFront(it)
		m.items[key] = it
	} else {
		m.lru.MoveToFront(it.el)
	}
	it.used = time.Now().Unix()

	if len(it.out) >= maxOutputsPerInput {
		memory.HookResults.Release(it.out[0].bytes)
		it.out = it.out[1:]
	}
	it.out = append(it.out, o)
	memory.HookResults.Add(o.bytes)

	limit := memory.HookResults.Effective()
	for limit > 0 && memory.HookResults.Used() > limit && m.lru.Len() > 1 {
		el := m.lru.Back()
		old := el.Value.(*outputItem)
		m.lru.Remove(el)
		delete(m.items, old.key)
		for _, x := range old.out {
			memory.HookResults.Release(x.bytes)
		}
	}
}

func (m *outputMemo) sweep(cutoff int64) int64 {
	m.mu.Lock()
	defer m.mu.Unlock()

	var freed int64
	for key, it := range m.items {
		if it.used >= cutoff {
			continue
		}

		m.lru.Remove(it.el)
		delete(m.items, key)
		for _, o := range it.out {
			memory.HookResults.Release(o.bytes)
			freed += o.bytes
		}
	}

	return freed
}

func (m *outputMemo) reset() {
	m.mu.Lock()
	defer m.mu.Unlock()

	for _, it := range m.items {
		for _, o := range it.out {
			memory.HookResults.Release(o.bytes)
		}
	}
	m.items = map[string]*outputItem{}
	m.lru.Init()
}

func diffOutput(before map[string][]byte, after map[string]any) (*output, bool) {
	o := &output{set: map[string]any{}}
	for k, v := range after {
		if !jsonSafe(v) {
			return nil, false
		}

		b, err := json.Marshal(v)
		if err != nil {
			return nil, false
		}

		if old, ok := before[k]; ok && bytes.Equal(old, b) {
			continue
		}

		o.set[k] = storage.DeepCopyDocument(map[string]any{"v": v})["v"]
		o.bytes += int64(len(k)+len(b)) * 3
	}

	for k := range before {
		if _, ok := after[k]; !ok {
			o.deleted = append(o.deleted, k)
		}
	}

	o.bytes += 128

	return o, true
}

func fieldBytes(entity map[string]any) map[string][]byte {
	out := make(map[string][]byte, len(entity))
	for k, v := range entity {
		b, _ := json.Marshal(v)
		out[k] = b
	}

	return out
}

func (o *output) apply(entity map[string]any) {
	for k, v := range o.set {
		entity[k] = storage.DeepCopyDocument(map[string]any{"v": v})["v"]
	}

	for _, k := range o.deleted {
		delete(entity, k)
	}
}

type determinism struct {
	broken atomic.Bool
}

func (d *determinism) install(vm *goja.Runtime) {
	vm.SetTimeSource(func() time.Time {
		d.broken.Store(true)
		return time.Now()
	})
	vm.SetRandSource(func() float64 {
		d.broken.Store(true)
		return rand.Float64()
	})
}

func ResetOutputMemo() {
	outputs.reset()
}

type OutputMemoStats struct {
	Hits   int64
	Misses int64
}

func OutputMemo() OutputMemoStats {
	return OutputMemoStats{Hits: outputs.hits.Load(), Misses: outputs.misses.Load()}
}
