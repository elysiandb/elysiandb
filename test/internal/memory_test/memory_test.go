package memory_test

import (
	"errors"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/memory"
)

func TestSharesFollowTheBudget(t *testing.T) {
	memory.Configure(1 << 40)
	t.Cleanup(func() { memory.Configure(0) })

	total := int64(0)
	for _, p := range memory.Pools() {
		if p.Limit() <= 0 {
			t.Fatalf("%s has no limit under an explicit budget", p.Name())
		}
		total += p.Limit()
	}

	if total > (1<<40)*65/100 {
		t.Fatalf("the shares should leave room for the runtime, they take %d of %d", total, int64(1<<40))
	}

	if memory.Current() != memory.Normal {
		t.Fatalf("a small heap under a huge budget is not under pressure, got %s", memory.Current())
	}
}

func TestWithoutABudgetNothingIsLimited(t *testing.T) {
	memory.Configure(0)

	if !memory.Requests.TryAcquire(1 << 50) {
		t.Fatalf("without a budget a request is never refused")
	}
	memory.Requests.Release(1 << 50)

	if memory.HookResults.Limit() <= 0 {
		t.Fatalf("hook results keep a default limit without a budget")
	}
}

func TestAcquireWaitsThenGivesUp(t *testing.T) {
	memory.Configure(1 << 40)
	t.Cleanup(func() { memory.Configure(0) })

	p := memory.Requests
	limit := p.Limit()
	if !p.TryAcquire(limit) {
		t.Fatalf("the first reservation always fits")
	}
	defer p.Release(limit)

	if p.TryAcquire(1) {
		t.Fatalf("a full pool refuses")
	}

	before := p.Refused()
	start := time.Now()
	if err := p.Acquire(1, 30*time.Millisecond, nil); !errors.Is(err, memory.ErrBusy) {
		t.Fatalf("a full pool ends in a refusal, got %v", err)
	}
	if time.Since(start) < 20*time.Millisecond {
		t.Fatalf("a request should wait before being refused")
	}
	if p.Refused() != before+1 {
		t.Fatalf("a refusal is counted")
	}

	stopped := errors.New("client left")
	if err := p.Acquire(1, time.Minute, func() error { return stopped }); !errors.Is(err, stopped) {
		t.Fatalf("a waiter stops when its request stops, got %v", err)
	}

	got := make(chan error, 1)
	go func() { got <- p.Acquire(1<<20, time.Minute, nil) }()
	time.Sleep(10 * time.Millisecond)
	p.Release(limit / 2)
	defer p.Add(limit / 2)
	select {
	case err := <-got:
		if err != nil {
			t.Fatalf("a waiter gets memory once it is released: %v", err)
		}
		p.Release(1 << 20)
	case <-time.After(5 * time.Second):
		t.Fatalf("a waiter was not woken by a release")
	}
}

func TestPressureFollowsTheProcess(t *testing.T) {
	memory.Configure(1 << 20)
	t.Cleanup(func() { memory.Configure(0) })

	memory.Sample()
	if memory.Current() != memory.Critical {
		t.Fatalf("a heap larger than the budget is critical, got %s", memory.Current())
	}

	if memory.Values.Effective() != memory.Values.Limit()/4 {
		t.Fatalf("critical pressure quarters the share of values")
	}

	if memory.HookResults.Effective() != memory.HookResults.Limit()/4 {
		t.Fatalf("critical pressure quarters the share of hook results")
	}
}
