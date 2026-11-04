package memory_test

import (
	"runtime"
	"testing"

	"github.com/taymour/elysiandb/internal/memory"
)

func TestLevelsFollowTheBudget(t *testing.T) {
	t.Cleanup(func() { memory.SetBudget(0) })

	runtime.GC()
	memory.SetBudget(0)
	if memory.Current() != memory.Normal || memory.UnderPressure() || memory.IsCritical() {
		t.Fatalf("without a budget there is no pressure, got level %d", memory.Current())
	}

	stats := memory.Snapshot()
	if stats.Budget != 0 || stats.Live <= 0 || stats.Used <= 0 {
		t.Fatalf("the sample reads the heap, got %+v", stats)
	}

	memory.SetBudget(1)
	if memory.Current() != memory.Critical || !memory.UnderPressure() || !memory.IsCritical() {
		t.Fatalf("a budget below the live heap is critical, got level %d", memory.Current())
	}
	if s := memory.Snapshot(); s.Budget != 1 || s.Level != memory.Critical {
		t.Fatalf("the snapshot reports the budget and the level, got %+v", s)
	}

	memory.SetBudget(1 << 50)
	if memory.Current() != memory.Normal {
		t.Fatalf("a large budget is not under pressure, got level %d", memory.Current())
	}

	memory.Start()
	memory.Start()
	memory.Sample()
	if memory.Current() != memory.Normal {
		t.Fatalf("the sampler keeps the level, got %d", memory.Current())
	}
}
