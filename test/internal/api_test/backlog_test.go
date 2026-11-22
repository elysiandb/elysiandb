package api_test

import (
	"fmt"
	"math/rand"
	"reflect"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/storage"
)

func openBackgroundStore(t *testing.T) *datastore.Engine {
	t.Helper()

	initIdxTestStore(t)
	if err := storage.Open(storage.ModeServer); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = storage.Close() })

	return storage.Engine()
}

func TestBulkWritesAnswerLikeAScan(t *testing.T) {
	e := openBackgroundStore(t)

	r := rand.New(rand.NewSource(11))
	bulk := func(from, n int) {
		docs := make([]map[string]any, 0, n)
		for i := from; i < from+n; i++ {
			docs = append(docs, randomDiffDoc(r, fmt.Sprintf("q%05d", i)))
		}
		api_storage.WriteListOfEntities("forms", docs)
	}

	bulk(0, 2000)
	for _, f := range []string{"a", "b", "m.x"} {
		for !e.EnsureIndex("forms", datastore.IndexSpec{Kind: datastore.IndexFilter, Field: f}) {
			time.Sleep(10 * time.Millisecond)
		}
	}
	for !e.EnsureIndex("forms", datastore.IndexSpec{Kind: datastore.IndexBitmap, Field: "list.x"}) {
		time.Sleep(10 * time.Millisecond)
	}
	for !e.EnsureIndex("forms", datastore.IndexSpec{Kind: datastore.IndexSort, Field: "a"}) {
		time.Sleep(10 * time.Millisecond)
	}
	for !e.EnsureIndex("forms", datastore.IndexSpec{Kind: datastore.IndexSearch}) {
		time.Sleep(10 * time.Millisecond)
	}
	for !e.EnsureIndex("forms", datastore.CompositeSpec("b", "a")) {
		time.Sleep(10 * time.Millisecond)
	}

	checkPlannerAgainstScan(t, r, "forms", 30)

	sawLag := false
	for round := 0; round < 12; round++ {
		bulk(2000+round*300, 300)
		sawLag = sawLag || e.IndexBacklogBytes() > 0

		for i := 0; i < 80; i++ {
			id := fmt.Sprintf("q%05d", r.Intn(2000))
			if i%5 == 0 {
				api_storage.DeleteEntityById("forms", id)
			} else {
				api_storage.WriteEntity("forms", randomDiffDoc(r, id))
			}
		}

		docs := make([]map[string]any, 0, 200)
		for i := 0; i < 200; i++ {
			docs = append(docs, randomDiffDoc(r, fmt.Sprintf("q%05d", r.Intn(2000))))
		}
		api_storage.WriteListOfEntities("forms", docs)

		checkPlannerAgainstScan(t, r, "forms", 25)

		if round%4 == 3 {
			if err := e.Checkpoint(); err != nil {
				t.Fatal(err)
			}
		}

		if round%4 == 1 {
			time.Sleep(450 * time.Millisecond)
			if e.IndexBacklogBytes() != 0 {
				t.Fatalf("the backlog should drain when writes stop: %d bytes", e.IndexBacklogBytes())
			}
		}
	}

	if !sawLag {
		t.Fatalf("bulk writes should go through the backlog")
	}
}

func TestReadsDuringBulkWritesNeverSeeAnOldIndex(t *testing.T) {
	e := openBackgroundStore(t)

	for !e.EnsureIndex("live", datastore.IndexSpec{Kind: datastore.IndexFilter, Field: "tag"}) {
		time.Sleep(10 * time.Millisecond)
	}

	var acked atomic.Int64
	var stop atomic.Bool
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for b := 0; !stop.Load(); b++ {
			docs := make([]map[string]any, 0, 100)
			for i := 0; i < 100; i++ {
				docs = append(docs, map[string]any{"id": fmt.Sprintf("d%d-%d", b, i), "tag": "x"})
			}
			api_storage.WriteListOfEntities("live", docs)
			acked.Add(100)
		}
	}()

	deadline := time.Now().Add(1500 * time.Millisecond)
	checks := 0
	for time.Now().Before(deadline) {
		before := acked.Load()
		res := api_storage.Plan(api_storage.ListQuery{Entity: "live", Filters: map[string]map[string]string{"tag": {"eq": "x"}}, CountOnly: true})
		after := acked.Load()
		if int64(res.Total) < before {
			t.Fatalf("a confirmed write is missing from a filter: %d found, %d confirmed", res.Total, before)
		}
		if int64(res.Total) > after+100 {
			t.Fatalf("the filter found too much: %d found, %d confirmed", res.Total, after)
		}
		checks++
	}
	stop.Store(true)
	wg.Wait()

	if checks == 0 {
		t.Fatal("no read ran")
	}

	res := api_storage.Plan(api_storage.ListQuery{Entity: "live", Filters: map[string]map[string]string{"tag": {"eq": "x"}}})
	got := []string{}
	for _, d := range res.Docs {
		got = append(got, d.ID())
	}
	want := []string{}
	snap := e.Snapshot("live")
	snap.Ascend(0, func(d datastore.Doc) bool {
		want = append(want, d.ID())
		return true
	})
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("%d found, %d stored", len(got), len(want))
	}
}

func TestAnIndexInBacklogIsRebuiltAfterACrash(t *testing.T) {
	initIdxTestStore(t)
	cfgFolder := storage.Engine().Folder()
	_ = storage.Close()

	open := func() *datastore.Engine {
		e, err := datastore.Open(datastore.Options{Folder: cfgFolder, Shards: 4, Background: true, Durability: 1})
		if err != nil {
			t.Fatal(err)
		}
		return e
	}

	e := open()
	spec := datastore.IndexSpec{Kind: datastore.IndexFilter, Field: "tag"}
	e.EnsureIndex("a", spec)
	for !e.EnsureIndex("a", spec) {
		time.Sleep(10 * time.Millisecond)
	}

	put := func(from int) {
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("a")
			for i := from; i < from+200; i++ {
				id := fmt.Sprintf("d%05d", i)
				if err := tx.Put("a", id, map[string]any{"id": id, "tag": "x"}); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}

	put(0)
	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}
	put(200)
	e.Abandon()

	e = open()
	defer e.Abandon()

	if n := e.Snapshot("a").Len(); n != 400 {
		t.Fatalf("%d documents", n)
	}

	for !e.EnsureIndex("a", spec) {
		time.Sleep(10 * time.Millisecond)
	}

	view, ok := e.IndexedSnapshot("a").Index(spec)
	if !ok {
		t.Fatal("the index is not usable")
	}

	found := 0
	view.ScanPrefix(append(datastore.AppendEscaped([]byte{datastore.KeyLower}, "x"), 0, 1), func(string, uint32) bool {
		found++
		return true
	})
	if found != 400 {
		t.Fatalf("the index holds %d of 400 documents", found)
	}
}

func TestLogReplayKeepsIndexesExact(t *testing.T) {
	e := openBackgroundStore(t)

	r := rand.New(rand.NewSource(23))
	docs := make([]map[string]any, 0, 3000)
	for i := 0; i < 3000; i++ {
		docs = append(docs, randomDiffDoc(r, fmt.Sprintf("q%05d", i)))
	}
	api_storage.WriteListOfEntities("forms", docs)

	specs := []datastore.IndexSpec{
		{Kind: datastore.IndexFilter, Field: "a"},
		{Kind: datastore.IndexFilter, Field: "b"},
		{Kind: datastore.IndexBitmap, Field: "list.x"},
		{Kind: datastore.IndexSort, Field: "a"},
		{Kind: datastore.IndexSearch},
		datastore.CompositeSpec("b", "a"),
	}
	for _, spec := range specs {
		for !e.EnsureIndex("forms", spec) {
			time.Sleep(10 * time.Millisecond)
		}
	}

	if _, ok := e.IndexedSnapshot("forms").Index(specs[0]); !ok {
		t.Fatal("the index is not usable")
	}

	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}

	for i := 0; i < 600; i++ {
		id := fmt.Sprintf("q%05d", r.Intn(3000))
		if i%6 == 0 {
			api_storage.DeleteEntityById("forms", id)
		} else {
			api_storage.WriteEntity("forms", randomDiffDoc(r, id))
		}
	}

	more := make([]map[string]any, 0, 500)
	for i := 3000; i < 3500; i++ {
		more = append(more, randomDiffDoc(r, fmt.Sprintf("q%05d", i)))
	}
	api_storage.WriteListOfEntities("forms", more)
	if err := e.FlushLog(); err != nil {
		t.Fatal(err)
	}

	e.Abandon()
	if err := storage.Open(storage.ModeServer); err != nil {
		t.Fatal(err)
	}
	e = storage.Engine()

	if e.ReplayedRecords() == 0 {
		t.Fatal("this test should replay the log")
	}

	if e.IndexBacklogBytes() != 0 {
		t.Fatalf("a replay should leave no backlog: %d bytes", e.IndexBacklogBytes())
	}

	for _, spec := range specs {
		for !e.EnsureIndex("forms", spec) {
			time.Sleep(10 * time.Millisecond)
		}
	}

	checkPlannerAgainstScan(t, r, "forms", 60)
}
