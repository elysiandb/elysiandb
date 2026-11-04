package bench_test

import (
	"fmt"
	"math/rand"
	"os"
	"runtime"
	"runtime/debug"
	"runtime/metrics"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/memory"
	"github.com/taymour/elysiandb/internal/storage"
)

var fillWords = strings.Fields("nuit ombre retour dernier royaume silence étoile mémoire voyage rivière jardin fantôme empire cendre lumière secret hiver orage miroir frontière légende promesse désert océan tempête dragon horizon labyrinthe chasseur prophétie révolte forteresse planète galaxie machine spectre colline rêve mirage sanctuaire vengeance sentinelle odyssée crépuscule aurore abîme écho sirène volcan")

func fillPhrase(r *rand.Rand, n int) string {
	out := make([]string, n)
	for i := range out {
		out[i] = fillWords[r.Intn(len(fillWords))]
	}
	return strings.Join(out, " ")
}

func fillWork(i int, r *rand.Rand) map[string]any {
	directors := []any{}
	for k := 0; k < 1+r.Intn(2); k++ {
		directors = append(directors, map[string]any{"@entity": "person", "id": fmt.Sprintf("p%06d", r.Intn(50000))})
	}
	return map[string]any{
		"id": fmt.Sprintf("w%08d", i), "title": "thanos " + fillPhrase(r, 3), "title_original": fillPhrase(r, 3),
		"title_uncertain": false, "kind": []string{"film", "serie", "documentaire"}[r.Intn(3)], "is_twist_work": r.Intn(10) == 0,
		"year": float64(1950 + r.Intn(76)), "runtime_min": float64(80 + r.Intn(110)), "franchise": fillWords[r.Intn(len(fillWords))],
		"synopsis_safe": fillPhrase(r, 40), "directors": directors, "warnings": []any{}, "streaming": []any{"netflix"},
	}
}

func fillEdition(i int, r *rand.Rand) map[string]any {
	return map[string]any{
		"id": fmt.Sprintf("e%08d", i), "label": "thanos " + fillPhrase(r, 3), "supports": []any{"bluray"}, "support_uncertain": false,
		"state": "ouvert", "vostfr_ok": "oui", "verified_on_arrival": r.Intn(2) == 0,
		"works":    []any{map[string]any{"@entity": "work", "id": fmt.Sprintf("w%08d", r.Intn(1000000))}},
		"location": []string{"pile_prioritaire", "etagere", "cave"}[r.Intn(3)], "region_label": "edition_francaise", "publisher": "studio",
		"audio": []any{"fr", "en"}, "subtitles": []any{"fr"}, "disc_count": float64(1 + r.Intn(3)), "ean": fmt.Sprintf("%013d", r.Int63n(9999999999999)), "bonus": fillPhrase(r, 15),
	}
}

func cpuSeconds(name string) float64 {
	s := []metrics.Sample{{Name: name}}
	metrics.Read(s)
	return s[0].Value.Float64()
}

func TestFillWithAMemoryBudget(t *testing.T) {
	total, _ := strconv.Atoi(os.Getenv("ELYSIAN_FILL_DOCS"))
	if total <= 0 {
		t.Skip("set ELYSIAN_FILL_DOCS (for example 3200000) to fill two entities under a 4 GB budget")
	}

	cfg := config(t.TempDir())
	cfg.Store.FlushIntervalSeconds = 5
	cfg.Store.Memory.LimitMB = 4096
	cfg.Store.Shards = 512
	globals.SetConfig(cfg)
	boot.ApplyMemoryLimit()
	t.Cleanup(func() {
		debug.SetMemoryLimit(-1)
		memory.SetBudget(0)
	})
	if err := storage.Open(storage.ModeServer); err != nil {
		t.Fatal(err)
	}
	defer storage.Close()

	seed := rand.New(rand.NewSource(1))
	api_storage.WriteListOfEntities("work", []map[string]any{fillWork(-1, seed)})
	api_storage.WriteListOfEntities("edition", []map[string]any{fillEdition(-1, seed)})
	for _, f := range []string{"title", "year", "runtime_min"} {
		api_storage.ListEntities("work", 1, 0, f, true, nil, "", "")
	}
	for _, f := range []map[string]map[string]string{{"kind": {"eq": "film"}}, {"directors.id": {"eq": "p000001"}}, {"is_twist_work": {"eq": "true"}}} {
		api_storage.ListEntities("work", 1, 0, "", true, f, "", "")
	}
	for _, f := range []map[string]map[string]string{{"location": {"eq": "cave"}}, {"works.id": {"eq": "w00000001"}}} {
		api_storage.ListEntities("edition", 1, 0, "", true, f, "", "")
	}
	api_storage.ListEntities("work", 1, 0, "", true, nil, "tempête", "")
	api_storage.ListEntities("edition", 1, 0, "", true, nil, "tempête", "")
	time.Sleep(time.Second)

	var done atomic.Int64
	var wg sync.WaitGroup
	start := time.Now()
	for w := 0; w < 4; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			r := rand.New(rand.NewSource(int64(w + 10)))
			entity := []string{"work", "edition"}[w%2]
			for {
				base := int(done.Add(500)) - 500
				if base >= total {
					return
				}
				batch := make([]map[string]any, 0, 500)
				for i := base; i < base+500; i++ {
					if entity == "work" {
						batch = append(batch, fillWork(i, r))
					} else {
						batch = append(batch, fillEdition(i, r))
					}
				}
				api_storage.WriteListOfEntities(entity, batch)
			}
		}(w)
	}

	step := 100000
	next := step
	lastT, lastGC, lastCPU := start, cpuSeconds("/cpu/classes/gc/total:cpu-seconds"), cpuSeconds("/cpu/classes/total:cpu-seconds")
	var peak uint64
	for {
		time.Sleep(50 * time.Millisecond)
		var ms runtime.MemStats
		runtime.ReadMemStats(&ms)
		peak = max(peak, ms.Sys-ms.HeapReleased)

		n := int(done.Load())
		if n >= next || n >= total {
			now := time.Now()
			gc, cpu := cpuSeconds("/cpu/classes/gc/total:cpu-seconds"), cpuSeconds("/cpu/classes/total:cpu-seconds")
			st := storage.Engine().Stats()
			t.Logf("%9d docs %7.0f docs/s  go memory %5d MB  gc %3.0f%% of cpu  last flush %6.0f ms  block cache %5d MB  compaction debt %5d MB",
				n, float64(step)/now.Sub(lastT).Seconds(), (ms.Sys-ms.HeapReleased)>>20, 100*(gc-lastGC)/max(cpu-lastCPU, 1e-9), st.LastFlushMs, st.BlockCacheBytes>>20, st.CompactionDebt>>20)
			lastT, lastGC, lastCPU = now, gc, cpu
			next += step
		}
		if n >= total {
			break
		}
	}
	wg.Wait()
	t.Logf("fill: %d documents in %s, peak Go memory %d MB for a budget of %d MB", total, time.Since(start).Round(time.Second), peak>>20, memory.Budget()>>20)

	t0 := time.Now()
	if err := storage.Checkpoint(); err != nil {
		t.Fatal(err)
	}
	t.Logf("checkpoint after the fill: %s", time.Since(t0).Round(time.Millisecond))

	api_storage.DeleteAllEntities("work")
	api_storage.DeleteAllEntities("edition")
	time.Sleep(5 * time.Second)
	runtime.GC()
	var ms runtime.MemStats
	runtime.ReadMemStats(&ms)
	t.Logf("after deleting everything: heap %d MB, Go memory kept from the system %d MB", ms.HeapAlloc>>20, (ms.Sys-ms.HeapReleased)>>20)
}
