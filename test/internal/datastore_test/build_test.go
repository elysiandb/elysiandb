package datastore_test

import (
	"fmt"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
)

func openBackground(t *testing.T) *datastore.Engine {
	t.Helper()

	opts := options(t.TempDir())
	opts.Background = true
	opts.IndexWorkers = 2
	opts.SegmentSize = 0
	e, err := datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = e.Close() })

	return e
}

func putDocs(t *testing.T, e *datastore.Engine, entity string, from, to int, doc func(i int) map[string]any) {
	t.Helper()

	const batch = 500
	for start := from; start < to; start += batch {
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType(entity)
			for i := start; i < min(start+batch, to); i++ {
				d := doc(i)
				if err := tx.Put(entity, d["id"].(string), d); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
}

func entries(e *datastore.Engine, entity string, spec datastore.IndexSpec) ([]string, bool) {
	view, ok := e.Snapshot(entity).Index(spec)
	if !ok {
		return nil, false
	}

	out := []string{}
	view.ScanRange(nil, nil, func(key string, _ uint32) bool {
		out = append(out, key)
		return true
	})

	return out, true
}

func waitReady(t *testing.T, e *datastore.Engine, entity string, specs ...datastore.IndexSpec) {
	t.Helper()

	deadline := time.Now().Add(20 * time.Second)
	for {
		ready := e.IndexBacklog() == 0
		for _, spec := range specs {
			if _, ok := e.Snapshot(entity).Index(spec); !ok {
				ready = false
			}
		}

		if ready {
			return
		}

		if time.Now().After(deadline) {
			t.Fatalf("the indexes of %s never became ready, backlog %d", entity, e.IndexBacklog())
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func workDoc(i int) map[string]any {
	return map[string]any{"id": fmt.Sprintf("w%05d", i), "a": float64(i % 100), "b": fmt.Sprintf("tag%d", i%13), "c": i%2 == 0, "text": fmt.Sprintf("alpha%05d", i)}
}

func TestBatchedIndexBuildFollowsConcurrentWrites(t *testing.T) {
	e := openBackground(t)
	putDocs(t, e, "w", 0, 20000, workDoc)

	specs := []datastore.IndexSpec{
		{Kind: datastore.IndexSort, Field: "a"},
		{Kind: datastore.IndexFilter, Field: "b"},
		{Kind: datastore.IndexFilter, Field: "c"},
		{Kind: datastore.IndexSort, Field: "text"},
	}

	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for i := 0; i < 300; i++ {
			_ = e.Update(func(tx *datastore.Tx) error {
				id := fmt.Sprintf("w%05d", i*37%20000)
				switch i % 3 {
				case 0:
					return tx.Put("w", id, map[string]any{"id": id, "a": float64(1000 + i), "b": "moved", "c": i%2 == 1, "text": "omega"})
				case 1:
					tx.Delete("w", id)
					return nil
				default:
					d := workDoc(20000 + i)
					return tx.Put("w", d["id"].(string), d)
				}
			})
		}
	}()

	for _, spec := range specs {
		e.EnsureIndex("w", spec)
	}
	e.SearchCandidates("w", "*omega*")
	wg.Wait()
	waitReady(t, e, "w", specs...)

	built := map[string][]string{}
	for _, spec := range specs {
		built[spec.Name()], _ = entries(e, "w", spec)
	}

	searched, ok := e.SearchCandidates("w", "*omega*")
	if !ok {
		t.Fatalf("the search index should be ready after the batch")
	}
	e.Snapshot("w").Ascend(0, func(d datastore.Doc) bool {
		doc, _ := d.Decode()
		if doc["text"] == "omega" && !searched.Has(d.Seq()) {
			t.Fatalf("the search index misses %s", d.ID())
		}
		return true
	})

	if n := e.DropIdleIndexes(-time.Minute); n != len(specs) {
		t.Fatalf("expected %d released indexes, got %d", len(specs), n)
	}
	for _, spec := range specs {
		e.EnsureIndex("w", spec)
	}
	waitReady(t, e, "w", specs...)

	for _, spec := range specs {
		quiet, _ := entries(e, "w", spec)
		if !reflect.DeepEqual(built[spec.Name()], quiet) {
			t.Fatalf("%s built under writes differs from a quiet rebuild (%d vs %d entries)", spec.Name(), len(built[spec.Name()]), len(quiet))
		}
	}
}

func TestIdleIndexIsReleasedAndRebuiltOnUse(t *testing.T) {
	e := openBackground(t)
	putDocs(t, e, "idle", 0, 3000, workDoc)

	spec := datastore.IndexSpec{Kind: datastore.IndexSort, Field: "a"}
	e.EnsureIndex("idle", spec)
	waitReady(t, e, "idle", spec)

	if n := e.DropIdleIndexes(time.Hour); n != 0 {
		t.Fatalf("an index used just now is kept, released %d", n)
	}

	if n := e.DropIdleIndexes(-time.Minute); n != 1 {
		t.Fatalf("an idle index is released, released %d", n)
	}

	snap := e.Snapshot("idle")
	if !snap.HasIndex(spec) || !snap.IndexDormant(spec) || e.IndexBacklog() != 0 {
		t.Fatalf("a released index stays registered, dormant, and out of the backlog")
	}
	if _, ok := snap.Index(spec); ok {
		t.Fatalf("a released index is not served")
	}

	putDocs(t, e, "idle", 3000, 3001, func(i int) map[string]any {
		return map[string]any{"id": "late", "a": -5.0}
	})

	e.EnsureIndex("idle", spec)
	waitReady(t, e, "idle", spec)

	got, _ := entries(e, "idle", spec)
	if len(got) != 3001 {
		t.Fatalf("the rebuilt index has %d entries, want 3001", len(got))
	}

	first := ""
	view, _ := e.Snapshot("idle").Index(spec)
	view.AscendAt(0, func(_ string, seq uint32) bool {
		d, _ := e.Snapshot("idle").BySeq(seq)
		first = d.ID()
		return false
	})
	if first != "late" {
		t.Fatalf("the rebuilt index includes the write made while it was released, first %s", first)
	}
}

func TestSmallEntityIndexIsBuiltAtOnce(t *testing.T) {
	e := openBackground(t)
	putDocs(t, e, "small", 0, 200, workDoc)

	spec := datastore.IndexSpec{Kind: datastore.IndexFilter, Field: "b"}
	if !e.EnsureIndex("small", spec) {
		t.Fatalf("a small entity gets its index without waiting for the background builders")
	}

	got, ok := entries(e, "small", spec)
	if !ok || len(got) == 0 || !strings.Contains(strings.Join(got, ""), "tag1") {
		t.Fatalf("the index of a small entity is ready and filled, got %d entries", len(got))
	}

	if _, ok := e.SearchCandidates("small", "*alpha*"); ok {
		t.Fatalf("the first search starts the index and scans")
	}
	if set, ok := e.SearchCandidates("small", "*alpha*"); !ok || set.Count() != 200 {
		t.Fatalf("the search index of a small entity is ready at the second search")
	}
}

func TestRestartRebuildsSearchIndexesAndKeepsReleasedIndexes(t *testing.T) {
	dir := t.TempDir()
	opts := options(dir)
	opts.Background = true
	opts.SegmentSize = 0

	e, err := datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}
	putDocs(t, e, "texts", 0, 3000, workDoc)

	released := datastore.IndexSpec{Kind: datastore.IndexSort, Field: "a"}
	kept := datastore.IndexSpec{Kind: datastore.IndexFilter, Field: "b"}
	e.EnsureIndex("texts", released)
	e.EnsureIndex("texts", kept)
	e.SearchCandidates("texts", "*alpha*")
	waitReady(t, e, "texts", released, kept)
	e.DropIdleIndexes(-time.Minute)
	e.EnsureIndex("texts", kept)
	waitReady(t, e, "texts", kept)
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}

	e, err = datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	snap := e.Snapshot("texts")
	if !snap.HasIndex(released) || !snap.IndexDormant(released) {
		t.Fatalf("a released index is still registered after a restart")
	}
	if _, ok := snap.Index(kept); !ok {
		t.Fatalf("a ready index is restored from the checkpoint")
	}

	deadline := time.Now().Add(10 * time.Second)
	for {
		if set, ok := e.SearchCandidates("texts", "*alpha*"); ok {
			if set.Count() != 3000 {
				t.Fatalf("the rebuilt search index has %d candidates, want 3000", set.Count())
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("the search index of a searched entity is rebuilt at startup")
		}
		time.Sleep(5 * time.Millisecond)
	}
}
