package api_storage

import (
	"errors"
	"sync/atomic"
	"time"
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
	parent    *Guard
	err       atomic.Pointer[error]
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
