package datastore_test

import (
	"fmt"
	"math/rand"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/durable"
)

type enduranceSample struct {
	goroutines int
	files      int
	logBytes   int64
	pageBytes  int64
	heap       uint64
	dirty      int64
}

func openDescriptors() int {
	entries, err := os.ReadDir("/dev/fd")
	if err != nil {
		return 0
	}

	return len(entries)
}

func folderBytes(dir string) int64 {
	var total int64
	entries, _ := os.ReadDir(dir)
	for _, entry := range entries {
		if info, err := entry.Info(); err == nil {
			total += info.Size()
		}
	}

	return total
}

func TestNothingDriftsAtConstantVolume(t *testing.T) {
	if testing.Short() {
		t.Skip("many rounds")
	}

	folder := t.TempDir()
	opts := options(folder)
	opts.Background = true
	opts.SegmentSize = 1 << 20
	opts.PageSegmentSize = 1 << 20
	opts.PageCacheBytes = 4 << 20
	opts.CacheBytes = 4 << 20
	opts.DirtyFlushBytes = 2 << 20
	opts.MaxLogBytes = 2 << 20
	opts.CheckpointEvery = 300 * time.Millisecond
	opts.RetiredFileGrace = 3 * time.Second

	e, err := datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	const documents = 1000
	r := rand.New(rand.NewSource(5))
	doc := func(i, round int) map[string]any {
		return map[string]any{"id": fmt.Sprintf("doc-%05d", i), "tag": fmt.Sprintf("t%d", r.Intn(40)), "rank": r.Intn(100000), "round": round, "pad": fmt.Sprintf("%0300d", i)}
	}

	const coldDocuments = 50
	writeAll := func(round int) {
		for from := 0; from < documents; from += 200 {
			if err := e.Update(func(tx *datastore.Tx) error {
				tx.EnsureType("steady")
				for i := from; i < from+200; i++ {
					if err := tx.Put("steady", fmt.Sprintf("doc-%05d", i), doc(i, round)); err != nil {
						return err
					}
				}
				if round == 0 {
					tx.EnsureType("cold")
					for i := from / 20; i < from/20+10; i++ {
						id := fmt.Sprintf("cold-%05d", i)
						if err := tx.Put("cold", id, map[string]any{"id": id, "pad": fmt.Sprintf("%0300d", i)}); err != nil {
							return err
						}
					}
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		}

		for i := 0; i < 100; i++ {
			at := r.Intn(documents)
			if err := e.Update(func(tx *datastore.Tx) error {
				return tx.Put("steady", fmt.Sprintf("doc-%05d", at), doc(at, round))
			}); err != nil {
				t.Fatal(err)
			}
		}

		for i := 0; i < 50; i++ {
			if err := e.KVPut(fmt.Sprintf("key-%d", i), []byte(fmt.Sprintf("%0200d", round)), 0); err != nil {
				t.Fatal(err)
			}
		}
	}

	writeAll(0)
	specs := []datastore.IndexSpec{{Kind: datastore.IndexFilter, Field: "tag"}, {Kind: datastore.IndexSort, Field: "rank"}}
	for _, spec := range specs {
		for !e.EnsureIndex("steady", spec) {
			time.Sleep(10 * time.Millisecond)
		}
	}

	measure := func() enduranceSample {
		time.Sleep(50 * time.Millisecond)
		runtime.GC()
		var mem runtime.MemStats
		runtime.ReadMemStats(&mem)

		return enduranceSample{
			goroutines: runtime.NumGoroutine(),
			files:      openDescriptors(),
			logBytes:   e.Stats().LogBytes,
			pageBytes:  folderBytes(filepath.Join(folder, durable.PagesFolder)),
			heap:       mem.HeapAlloc,
			dirty:      e.DirtyBytes(),
		}
	}

	const rounds = 45
	samples := make([]enduranceSample, 0, rounds)
	for round := 1; round <= rounds; round++ {
		began := time.Now()
		writeAll(round)

		snap := e.IndexedSnapshot("steady")
		if snap.Len() != documents {
			t.Fatalf("round %d: %d documents", round, snap.Len())
		}
		for _, spec := range specs {
			if _, ok := snap.Index(spec); !ok {
				for !e.EnsureIndex("steady", spec) {
					time.Sleep(10 * time.Millisecond)
				}
			}
		}

		if round%5 == 0 {
			view := e.PointInTime()
			count := 0
			view.Snapshots["steady"].ScanValues(func(_ datastore.Doc, _ []byte, err error) bool {
				if err != nil {
					t.Error(err)
					return false
				}
				count++
				return true
			})
			view.Release()
			if count != documents {
				t.Fatalf("round %d: the view holds %d documents", round, count)
			}
		}

		samples = append(samples, measure())
		time.Sleep(time.Second - time.Since(began))
	}

	average := func(part []enduranceSample, pick func(enduranceSample) float64) float64 {
		total := 0.0
		for _, s := range part {
			total += pick(s)
		}
		return total / float64(len(part))
	}

	middle, last := samples[rounds/3:2*rounds/3], samples[2*rounds/3:]
	checks := []struct {
		name  string
		pick  func(enduranceSample) float64
		ratio float64
		slack float64
	}{
		{"goroutines", func(s enduranceSample) float64 { return float64(s.goroutines) }, 1.1, 4},
		{"file descriptors", func(s enduranceSample) float64 { return float64(s.files) }, 1.1, 6},
		{"log size", func(s enduranceSample) float64 { return float64(s.logBytes) }, 1.3, 4 << 20},
		{"page files size", func(s enduranceSample) float64 { return float64(s.pageBytes) }, 1.3, 4 << 20},
		{"heap", func(s enduranceSample) float64 { return float64(s.heap) }, 1.3, 8 << 20},
	}

	for _, c := range checks {
		before, after := average(middle, c.pick), average(last, c.pick)
		t.Logf("%s: %.0f then %.0f", c.name, before, after)
		if after > before*c.ratio+c.slack {
			t.Errorf("%s drifts: %.0f then %.0f", c.name, before, after)
		}
	}

	if st := e.Stats(); st.CompactedSegments == 0 {
		t.Errorf("segments that keep a little live data should be compacted: %+v", st)
	}

	cold := e.Snapshot("cold")
	if cold.Len() != coldDocuments {
		t.Fatalf("%d documents that were never rewritten, %d expected", cold.Len(), coldDocuments)
	}
	count := 0
	cold.ScanValues(func(d datastore.Doc, _ []byte, err error) bool {
		if err != nil {
			t.Error(err)
			return false
		}
		count++
		return true
	})
	if count != coldDocuments {
		t.Fatalf("%d of %d documents can be read after compaction", count, coldDocuments)
	}
}
