package bench_test

import (
	"fmt"
	"math/rand"
	"os"
	"runtime"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func heapBytes() uint64 {
	runtime.GC()
	runtime.GC()
	var m runtime.MemStats
	runtime.ReadMemStats(&m)

	return m.HeapAlloc
}

func TestScaleReport(t *testing.T) {
	if os.Getenv("ELYSIAN_SCALE") == "" {
		t.Skip("set ELYSIAN_SCALE=1 to measure the engine on 60,000 documents")
	}

	folder := t.TempDir()
	globals.SetConfig(config(folder))
	storage.LoadDB()

	base := heapBytes()
	r := rand.New(rand.NewSource(7))
	total := 60000
	docs := make([]map[string]any, 0, total)
	for i := 0; i < total; i++ {
		docs = append(docs, work(i, r))
	}

	start := time.Now()
	for s := 0; s < total; s += 500 {
		api_storage.WriteListOfEntities("work", docs[s:s+500])
	}
	insert := time.Since(start)
	docs = nil

	api_storage.ListEntities("work", 1, 0, "year", true, nil, "", "")
	api_storage.ListEntities("work", 1, 0, "", true, map[string]map[string]string{"status": {"eq": "draft"}}, "", "")
	heap := heapBytes() - base

	t0 := time.Now()
	api_storage.DeleteEntityById("work", "w030000")
	deleteAt60k := time.Since(t0)

	if err := storage.Checkpoint(); err != nil {
		t.Fatal(err)
	}

	t0 = time.Now()
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	closeTime := time.Since(t0)

	t0 = time.Now()
	storage.LoadDB()
	restart := time.Since(t0)

	for i := 0; i < 2000; i++ {
		api_storage.WriteEntity("work", map[string]any{"id": fmt.Sprintf("tail-%d", i), "status": "draft", "year": float64(i)})
	}
	storage.Engine().Abandon()

	t0 = time.Now()
	storage.LoadDB()
	crashRestart := time.Since(t0)

	if got := api_storage.CountEntities("work"); got != total-1+2000 {
		t.Fatalf("expected %d documents after a crash, got %d", total-1+2000, got)
	}

	t0 = time.Now()
	api_storage.DeleteAllEntities("work")
	reset := time.Since(t0)
	_ = storage.Close()

	t.Logf("insert: %d docs in %s (%.0f docs/s)", total, insert.Round(time.Millisecond), float64(total)/insert.Seconds())
	t.Logf("heap after the fill with 2 indexes: %.1f MB (%.0f bytes per document)", float64(heap)/(1<<20), float64(heap)/float64(total))
	t.Logf("one delete at %d documents: %s", total, deleteAt60k)
	t.Logf("close with checkpoint: %s, restart after a clean stop: %s, restart after a crash with 2000 writes to replay: %s", closeTime.Round(time.Millisecond), restart.Round(time.Millisecond), crashRestart.Round(time.Millisecond))
	t.Logf("entity reset: %s", reset)
}
