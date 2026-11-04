package api_storage

import (
	"errors"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/memory"
)

var (
	ErrDeadline  = errors.New("request deadline exceeded")
	ErrAbandoned = errors.New("request abandoned by the client")
	ErrBusy      = errors.New("server busy")
)

type Guard struct {
	Deadline  time.Time
	Abandoned func() bool
	Admitted  bool
	Laned     bool
	parent    *Guard
	err       atomic.Pointer[error]
	reserved  atomic.Int64
	charged   atomic.Int64
}

const memoryChunk = 1 << 20

func (g *Guard) root() *Guard {
	for g != nil && g.parent != nil {
		g = g.parent
	}

	return g
}

func (g *Guard) Charge(n int64) error {
	r := g.root()
	if r == nil || n <= 0 || memory.Requests.Limit() <= 0 {
		return nil
	}

	used := r.charged.Add(n)
	if limit := memory.RequestCap(); limit > 0 && used > limit {
		memory.Requests.Refuse()
		return r.fail(ErrBusy)
	}

	for {
		reserved := r.reserved.Load()
		if used <= reserved {
			return nil
		}

		chunk := max(int64(memoryChunk), used-reserved)
		if err := memory.Requests.Acquire(chunk, heavyQueueTimeout(), r.Check); err != nil {
			if errors.Is(err, memory.ErrBusy) {
				return r.fail(ErrBusy)
			}
			return err
		}

		r.reserved.Add(chunk)
	}
}

func (g *Guard) Reserve(n int64) error {
	r := g.root()
	if r == nil || memory.Requests.Limit() <= 0 {
		return nil
	}

	for {
		reserved := r.reserved.Load()
		if reserved >= n {
			return nil
		}

		if err := memory.Requests.Acquire(n-reserved, heavyQueueTimeout(), r.Check); err != nil {
			if errors.Is(err, memory.ErrBusy) {
				return r.fail(ErrBusy)
			}
			return err
		}

		r.reserved.Add(n - reserved)
	}
}

func (g *Guard) Reject() error {
	if r := g.root(); r != nil {
		return r.fail(ErrBusy)
	}

	return ErrBusy
}

func (g *Guard) ReleaseMemory() {
	r := g.root()
	if r == nil {
		return
	}

	r.charged.Store(0)
	if n := r.reserved.Swap(0); n > 0 {
		memory.Requests.Release(n)
	}
}

func (g *Guard) Check() error {
	if g == nil {
		return nil
	}

	if err := g.err.Load(); err != nil {
		return *err
	}

	if !g.Deadline.IsZero() && time.Now().After(g.Deadline) {
		return g.fail(ErrDeadline)
	}

	if g.Abandoned != nil && g.Abandoned() {
		return g.fail(ErrAbandoned)
	}

	if err := g.parent.Check(); err != nil {
		return g.fail(err)
	}

	return nil
}

func (g *Guard) fail(err error) error {
	g.err.CompareAndSwap(nil, &err)

	return *g.err.Load()
}

func (g *Guard) Err() error {
	if g == nil {
		return nil
	}

	if err := g.err.Load(); err != nil {
		return *err
	}

	return nil
}

func (g *Guard) Until(deadline time.Time) *Guard {
	return &Guard{Deadline: deadline, Admitted: true, parent: g}
}

func (g *Guard) Stopped() bool {
	return g.Check() != nil
}
