package memory

import (
	"errors"
	"runtime/metrics"
	"sync"
	"sync/atomic"
	"time"
)

type Level int32

const (
	Normal Level = iota
	High
	Critical
)

func (l Level) String() string {
	switch l {
	case High:
		return "high"
	case Critical:
		return "critical"
	}

	return "normal"
}

var ErrBusy = errors.New("memory budget exhausted")

const (
	valuesShare   = 0.15
	pagesShare    = 0.15
	dirtyShare    = 0.04
	hooksShare    = 0.08
	buildsShare   = 0.06
	requestsShare = 0.12

	highShare     = 0.65
	criticalShare = 0.80

	defaultHookResults = 256 << 20
	minBuildBytes      = 64 << 20
	sampleInterval     = 100 * time.Millisecond
)

type Pool struct {
	name        string
	highPercent int
	limit       atomic.Int64
	used        atomic.Int64
	peak        atomic.Int64
	refused     atomic.Int64

	mu      sync.Mutex
	changed chan struct{}
}

func newPool(name string, highPercent int) *Pool {
	return &Pool{name: name, highPercent: highPercent, changed: make(chan struct{})}
}

func (p *Pool) Name() string {
	return p.name
}

func (p *Pool) Limit() int64 {
	return p.limit.Load()
}

func (p *Pool) Used() int64 {
	return p.used.Load()
}

func (p *Pool) Peak() int64 {
	return p.peak.Load()
}

func (p *Pool) Refused() int64 {
	return p.refused.Load()
}

func (p *Pool) setLimit(n int64) {
	p.limit.Store(n)
	p.signal()
}

func (p *Pool) Add(n int64) {
	if v := p.used.Add(n); v > p.peak.Load() {
		p.peak.Store(v)
	}

	if n < 0 {
		p.signal()
	}
}

func (p *Pool) Available() int64 {
	limit := p.Effective()
	if limit <= 0 {
		return 1 << 62
	}

	return limit - p.used.Load()
}

func (p *Pool) Effective() int64 {
	limit := p.limit.Load()
	if limit <= 0 {
		return 0
	}

	switch Current() {
	case High:
		return limit * int64(p.highPercent) / 100
	case Critical:
		return limit / 4
	}

	return limit
}

func (p *Pool) Set(n int64) {
	old := p.used.Swap(n)
	if n > p.peak.Load() {
		p.peak.Store(n)
	}

	if n < old {
		p.signal()
	}
}

func (p *Pool) TryAcquire(n int64) bool {
	limit := p.Effective()
	for {
		used := p.used.Load()
		if limit > 0 && used > 0 && used+n > limit {
			return false
		}

		if p.used.CompareAndSwap(used, used+n) {
			if used+n > p.peak.Load() {
				p.peak.Store(used + n)
			}
			return true
		}
	}
}

func (p *Pool) Acquire(n int64, timeout time.Duration, stop func() error) error {
	if p.TryAcquire(n) {
		return nil
	}

	deadline := time.Now().Add(timeout)
	for {
		p.mu.Lock()
		ch := p.changed
		p.mu.Unlock()

		if p.TryAcquire(n) {
			return nil
		}

		wait := time.Until(deadline)
		if timeout >= 0 && wait <= 0 {
			p.refused.Add(1)
			return ErrBusy
		}

		if wait > 20*time.Millisecond || timeout < 0 {
			wait = 20 * time.Millisecond
		}

		select {
		case <-ch:
		case <-time.After(wait):
		}

		if stop != nil {
			if err := stop(); err != nil {
				return err
			}
		}
	}
}

func (p *Pool) Release(n int64) {
	p.Add(-n)
}

func (p *Pool) signal() {
	p.mu.Lock()
	close(p.changed)
	p.changed = make(chan struct{})
	p.mu.Unlock()
}

var (
	Values      = newPool("values", 50)
	Pages       = newPool("pages", 50)
	Dirty       = newPool("dirty_pages", 100)
	HookResults = newPool("hook_results", 100)
	Builds      = newPool("index_builds", 50)
	Requests    = newPool("requests", 50)

	pools = []*Pool{Values, Pages, Dirty, HookResults, Builds, Requests}
)

func Pools() []*Pool {
	return pools
}

var (
	budget atomic.Int64
	level  atomic.Int32
	live   atomic.Int64
	total  atomic.Int64

	listenersMu sync.Mutex
	listeners   []func(Level)

	samplerMu sync.Mutex
	stopCh    chan struct{}
	samples   = []metrics.Sample{
		{Name: "/gc/heap/live:bytes"},
		{Name: "/memory/classes/total:bytes"},
		{Name: "/memory/classes/heap/released:bytes"},
	}
)

func Configure(bytes int64) {
	budget.Store(bytes)

	if bytes <= 0 {
		for _, p := range pools {
			p.setLimit(0)
		}
		HookResults.setLimit(defaultHookResults)
		Sample()
		return
	}

	Values.setLimit(share(bytes, valuesShare))
	Pages.setLimit(share(bytes, pagesShare))
	Dirty.setLimit(share(bytes, dirtyShare))
	HookResults.setLimit(share(bytes, hooksShare))
	Builds.setLimit(max(share(bytes, buildsShare), minBuildBytes))
	Requests.setLimit(share(bytes, requestsShare))
	Sample()
}

func share(b int64, s float64) int64 {
	return int64(float64(b) * s)
}

func Budget() int64 {
	return budget.Load()
}

func Current() Level {
	return Level(level.Load())
}

func Live() int64 {
	return live.Load()
}

func Total() int64 {
	return total.Load()
}

func OnLevel(fn func(Level)) {
	listenersMu.Lock()
	listeners = append(listeners, fn)
	listenersMu.Unlock()
}

func Sample() {
	samplerMu.Lock()
	metrics.Read(samples)
	l := int64(samples[0].Value.Uint64())
	t := int64(samples[1].Value.Uint64()) - int64(samples[2].Value.Uint64())
	samplerMu.Unlock()

	live.Store(l)
	total.Store(t)

	next := levelFor(budget.Load(), l)
	prev := Level(level.Swap(int32(next)))
	if next == prev {
		return
	}

	for _, p := range pools {
		p.signal()
	}

	if next > prev {
		listenersMu.Lock()
		fns := append([]func(Level){}, listeners...)
		listenersMu.Unlock()
		for _, fn := range fns {
			fn(next)
		}
	}
}

func levelFor(b, l int64) Level {
	if b <= 0 {
		return Normal
	}

	switch {
	case float64(l) >= criticalShare*float64(b):
		return Critical
	case float64(l) >= highShare*float64(b):
		return High
	}

	return Normal
}

func Start() {
	samplerMu.Lock()
	defer samplerMu.Unlock()

	if stopCh != nil {
		return
	}

	stop := make(chan struct{})
	stopCh = stop
	go func() {
		ticker := time.NewTicker(sampleInterval)
		defer ticker.Stop()
		for {
			select {
			case <-stop:
				return
			case <-ticker.C:
				Sample()
			}
		}
	}()
}

func Stop() {
	samplerMu.Lock()
	defer samplerMu.Unlock()

	if stopCh != nil {
		close(stopCh)
		stopCh = nil
	}
}
