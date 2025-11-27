package engine_test

import (
	"sync"
	"testing"

	"github.com/taymour/elysiandb/internal/engine"
)

func TestArenaAllocBasic(t *testing.T) {
	a := engine.NewArena(32)
	b := a.Alloc(10)
	if len(b) != 10 {
		t.Fatalf("expected len 10, got %d", len(b))
	}
	if cap(b) < 10 {
		t.Fatalf("expected cap >= 10, got %d", cap(b))
	}
}

func TestArenaAllocMultiple(t *testing.T) {
	a := engine.NewArena(32)
	_ = a.Alloc(10)
	b := a.Alloc(5)
	if len(b) != 5 {
		t.Fatalf("expected len 5, got %d", len(b))
	}
	if a.CurChunkCount() != 1 {
		t.Fatalf("expected 1 chunk, got %d", a.CurChunkCount())
	}
}

func TestArenaAllocForcesNewChunk(t *testing.T) {
	a := engine.NewArena(16)
	_ = a.Alloc(10)
	_ = a.Alloc(10)
	if a.CurChunkCount() != 2 {
		t.Fatalf("expected 2 chunks, got %d", a.CurChunkCount())
	}
}

func TestArenaAllocLarge(t *testing.T) {
	a := engine.NewArena(16)
	b := a.Alloc(100)
	if len(b) != 100 {
		t.Fatalf("expected len 100, got %d", len(b))
	}
	if cap(b) < 100 {
		t.Fatalf("expected cap >= 100, got %d", cap(b))
	}
	if a.CurChunkCount() != 2 {
		t.Fatalf("expected 2 chunks, got %d", a.CurChunkCount())
	}
}

func TestArenaReset(t *testing.T) {
	a := engine.NewArena(32)
	_ = a.Alloc(10)
	_ = a.Alloc(10)
	a.Reset()
	if a.CurChunkCount() != 1 {
		t.Fatalf("expected 1 chunk after reset, got %d", a.CurChunkCount())
	}
	b := a.Alloc(20)
	if len(b) != 20 {
		t.Fatalf("expected len 20, got %d", len(b))
	}
}

func TestArenaConcurrentAlloc(t *testing.T) {
	a := engine.NewArena(64)
	var wg sync.WaitGroup
	for i := 0; i < 50; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_ = a.Alloc(8)
		}()
	}
	wg.Wait()
	if a.CurChunkCount() == 0 {
		t.Fatalf("expected at least 1 chunk, got 0")
	}
}
