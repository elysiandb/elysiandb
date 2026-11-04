package datastore_test

import (
	"fmt"
	"slices"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
)

func TestClearAllAndNewEntitiesStayConsistent(t *testing.T) {
	e, err := datastore.Open(options(t.TempDir()))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	for round := 0; round < 200; round++ {
		name := fmt.Sprintf("e%03d", round)
		var wg sync.WaitGroup
		wg.Add(2)
		go func() {
			defer wg.Done()
			_ = e.Update(func(tx *datastore.Tx) error {
				tx.EnsureType(name)
				return tx.Put(name, "d", map[string]any{"id": "d"})
			})
		}()
		go func() {
			defer wg.Done()
			_ = e.Update(func(tx *datastore.Tx) error {
				tx.ClearAll()
				return nil
			})
		}()
		wg.Wait()

		types := e.Types()
		for i := 0; i <= round; i++ {
			entity := fmt.Sprintf("e%03d", i)
			if n := e.Snapshot(entity).Len(); n > 0 && !slices.Contains(types, entity) {
				t.Fatalf("round %d: %s holds %d documents but its type was cleared", round, entity, n)
			}
		}
	}
}

func TestCloseWaitsForReadersAndRefusesLaterReads(t *testing.T) {
	e, err := datastore.Open(options(t.TempDir()))
	if err != nil {
		t.Fatal(err)
	}

	putDocs(t, e, "r", 0, 2000, workDoc)
	if err := e.KVPut("k", []byte("v"), 0); err != nil {
		t.Fatal(err)
	}

	var stop atomic.Bool
	var reads atomic.Int64
	var wg sync.WaitGroup
	for g := 0; g < 8; g++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for !stop.Load() {
				snap := e.Snapshot("r")
				snap.Ascend(0, func(d datastore.Doc) bool {
					_, _ = d.Bytes()
					return d.Seq() < 50
				})
				if d, ok := snap.Get("w00010"); ok {
					_, _ = d.Decode()
				}
				_, _ = e.Read("r", "w00020")
				_, _, _ = e.KVGet("k")
				_ = e.Stats()
				reads.Add(1)
			}
		}()
	}

	time.Sleep(50 * time.Millisecond)
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
	time.Sleep(20 * time.Millisecond)
	stop.Store(true)
	wg.Wait()

	if reads.Load() == 0 {
		t.Fatalf("the readers ran")
	}

	if _, ok := e.Read("r", "w00020"); ok {
		t.Fatalf("a closed engine reads nothing")
	}
	if e.Snapshot("r").Len() != 0 {
		t.Fatalf("a closed engine gives empty snapshots")
	}
}
