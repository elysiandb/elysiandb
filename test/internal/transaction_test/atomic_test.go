package transaction_test

import (
	"os"
	"path/filepath"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transaction"
)

func commitThreeOperations(t *testing.T) {
	t.Helper()

	tx := transaction.BeginTransaction()
	for _, op := range []transaction.TxOperation{
		{Kind: "write", Entity: "acct", Data: map[string]any{"id": "a", "balance": 10.0}},
		{Kind: "update", Entity: "acct", ID: "seed", Data: map[string]any{"balance": 90.0}},
		{Kind: "delete", Entity: "acct", ID: "old"},
	} {
		if err := transaction.AddOperation(tx.ID, op); err != nil {
			t.Fatal(err)
		}
	}

	if err := transaction.CommitTransaction(tx.ID); err != nil {
		t.Fatal(err)
	}
}

func lastSegment(t *testing.T, folder string) string {
	t.Helper()

	entries, err := os.ReadDir(filepath.Join(folder, datastore.LogFolder))
	if err != nil {
		t.Fatal(err)
	}

	last := ""
	for _, e := range entries {
		last = e.Name()
	}

	return filepath.Join(folder, datastore.LogFolder, last)
}

func TestCommitIsAtomicAcrossACrash(t *testing.T) {
	for _, torn := range []bool{false, true} {
		folder := t.TempDir()
		globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: folder, Shards: 4, CrashRecovery: configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 100}}})
		storage.LoadDB()

		api_storage.WriteEntity("acct", map[string]any{"id": "seed", "balance": 100.0})
		api_storage.WriteEntity("acct", map[string]any{"id": "old", "balance": 0.0})
		if err := storage.Checkpoint(); err != nil {
			t.Fatal(err)
		}

		path := lastSegment(t, folder)
		before, _ := os.Stat(path)

		commitThreeOperations(t)
		storage.Engine().Abandon()

		after, _ := os.Stat(path)
		if after.Size() <= before.Size() {
			t.Fatalf("the commit should be in the log")
		}

		if torn {
			if err := os.Truncate(path, after.Size()-3); err != nil {
				t.Fatal(err)
			}
		}

		storage.LoadDB()

		a := api_storage.ReadEntityById("acct", "a")
		seed := api_storage.ReadEntityById("acct", "seed")
		old := api_storage.ReadEntityById("acct", "old")

		if torn {
			if a != nil || seed["balance"] != 100.0 || old == nil {
				t.Fatalf("a torn commit is dropped as a whole, got a=%v seed=%v old=%v", a, seed, old)
			}
		} else {
			if a == nil || seed["balance"] != 90.0 || old != nil {
				t.Fatalf("a complete commit is replayed as a whole, got a=%v seed=%v old=%v", a, seed, old)
			}
		}

		_ = storage.Close()
	}
}

func TestCommitIsOneBatch(t *testing.T) {
	folder := t.TempDir()
	globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: folder, Shards: 4, CrashRecovery: configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 100}}})
	storage.LoadDB()

	api_storage.WriteEntity("acct", map[string]any{"id": "seed", "balance": 100.0})
	api_storage.WriteEntity("acct", map[string]any{"id": "old", "balance": 0.0})
	if err := storage.Checkpoint(); err != nil {
		t.Fatal(err)
	}

	commitThreeOperations(t)
	storage.Engine().Abandon()

	j, err := journal.Open(filepath.Join(folder, datastore.LogFolder), journal.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer j.CloseWithoutWriter()

	var batches [][]journal.Record
	if _, err := j.Replay(journal.Position{Segment: 1}, func(recs []journal.Record) error {
		batches = append(batches, recs)
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	last := batches[len(batches)-1]
	kinds := map[journal.Kind]int{}
	for _, r := range last {
		kinds[r.Kind]++
	}

	if kinds[journal.KindDocPut] < 2 || kinds[journal.KindDocDelete] != 1 {
		t.Fatalf("the three operations should be in the last batch, got %v", kinds)
	}
}
