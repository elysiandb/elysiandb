package transaction_test

import (
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
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

	entries, err := os.ReadDir(filepath.Join(folder, datastore.PebbleFolder))
	if err != nil {
		t.Fatal(err)
	}

	logs := []string{}
	for _, e := range entries {
		if strings.HasSuffix(e.Name(), ".log") {
			logs = append(logs, e.Name())
		}
	}
	sort.Strings(logs)
	if len(logs) == 0 {
		t.Fatalf("expected a write-ahead log in %s", folder)
	}

	return filepath.Join(folder, datastore.PebbleFolder, logs[len(logs)-1])
}

func copyTree(t *testing.T, src string) string {
	t.Helper()

	dst := t.TempDir()
	err := filepath.WalkDir(src, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, _ := filepath.Rel(src, path)
		if d.IsDir() {
			return os.MkdirAll(filepath.Join(dst, rel), 0o755)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		return os.WriteFile(filepath.Join(dst, rel), data, 0o600)
	})
	if err != nil {
		t.Fatal(err)
	}

	return dst
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
			if err := os.Truncate(path, before.Size()+(after.Size()-before.Size())/2); err != nil {
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

	path := lastSegment(t, folder)
	before, _ := os.Stat(path)
	commitThreeOperations(t)
	storage.Engine().Abandon()
	after, _ := os.Stat(path)

	grown := after.Size() - before.Size()
	if grown <= 0 {
		t.Fatalf("the commit should be in the log")
	}

	rel, _ := filepath.Rel(folder, path)
	outcomes := map[string]int{}
	for cut := int64(0); cut <= grown; cut += max(1, grown/16) {
		crashed := copyTree(t, folder)
		if err := os.Truncate(filepath.Join(crashed, rel), after.Size()-cut); err != nil {
			t.Fatal(err)
		}

		globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: crashed, Shards: 4, CrashRecovery: configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 100}}})
		storage.LoadDB()

		a := api_storage.ReadEntityById("acct", "a")
		seed := api_storage.ReadEntityById("acct", "seed")
		old := api_storage.ReadEntityById("acct", "old")

		switch {
		case a != nil && seed["balance"] == 90.0 && old == nil:
			outcomes["all"]++
		case a == nil && seed["balance"] == 100.0 && old != nil:
			outcomes["none"]++
		default:
			t.Fatalf("cut %d of %d: a commit is replayed as a whole or not at all, got a=%v seed=%v old=%v", cut, grown, a, seed, old)
		}

		_ = storage.Close()
	}

	if outcomes["all"] == 0 || outcomes["none"] == 0 {
		t.Fatalf("expected complete and torn commits, got %v", outcomes)
	}
}
