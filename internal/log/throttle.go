package log

import (
	"sync"
	"time"
)

const throttleMaxKeys = 10_000

type Throttle struct {
	mu       sync.Mutex
	interval time.Duration
	entries  map[string]*throttleEntry
	now      func() time.Time
}

type throttleEntry struct {
	last       time.Time
	suppressed int
}

func NewThrottle(interval time.Duration) *Throttle {
	return &Throttle{interval: interval, entries: map[string]*throttleEntry{}, now: time.Now}
}

func (t *Throttle) SetClock(now func() time.Time) {
	t.mu.Lock()
	defer t.mu.Unlock()

	t.now = now
}

func (t *Throttle) Allow(key string) (int, bool) {
	t.mu.Lock()
	defer t.mu.Unlock()

	now := t.now()
	e, ok := t.entries[key]
	if !ok {
		t.makeRoom(now)
		t.entries[key] = &throttleEntry{last: now}
		return 0, true
	}

	if now.Sub(e.last) < t.interval {
		e.suppressed++
		return 0, false
	}

	suppressed := e.suppressed
	e.last = now
	e.suppressed = 0

	return suppressed, true
}

func (t *Throttle) Reset(key string) int {
	t.mu.Lock()
	defer t.mu.Unlock()

	e, ok := t.entries[key]
	if !ok {
		return 0
	}

	delete(t.entries, key)

	return e.suppressed
}

func (t *Throttle) makeRoom(now time.Time) {
	if len(t.entries) < throttleMaxKeys {
		return
	}

	for k, e := range t.entries {
		if now.Sub(e.last) >= t.interval {
			delete(t.entries, k)
		}
	}

	if len(t.entries) >= throttleMaxKeys {
		t.entries = map[string]*throttleEntry{}
	}
}

func WithSuppressed(args []any, suppressed int) []any {
	if suppressed > 0 {
		return append(args, "suppressed", suppressed)
	}

	return args
}
