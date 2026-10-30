package internal_boot_test

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/storage"
)

const (
	phaseEnv  = "ELYSIANDB_BOOT_TEST_PHASE"
	folderEnv = "ELYSIANDB_BOOT_TEST_FOLDER"
)

func bootInternal(t *testing.T, folder string) {
	t.Helper()

	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder:        folder,
			Shards:        8,
			CrashRecovery: configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 10},
		},
		Api: configuration.ApiConfig{Index: configuration.ApiIndexConfig{Workers: 1}},
	})

	boot.InitDB()
}

func runPhase(t *testing.T, phase, folder string) {
	t.Helper()

	cmd := exec.Command(os.Args[0], "-test.run=^TestInternalEngineBootLoadsAndRecoversBothStores$", "-test.count=1")
	cmd.Env = append(os.Environ(), phaseEnv+"="+phase, folderEnv+"="+folder)
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("phase %s failed: %v\n%s", phase, err, out)
	}
}

func TestInternalEngineBootLoadsAndRecoversBothStores(t *testing.T) {
	switch os.Getenv(phaseEnv) {
	case "write":
		writeThenCrash(t, os.Getenv(folderEnv))
	case "recover":
		recoverAfterCrash(t, os.Getenv(folderEnv))
	default:
		folder := t.TempDir()
		runPhase(t, "write", folder)
		runPhase(t, "recover", folder)
	}
}

func writeThenCrash(t *testing.T, folder string) {
	bootInternal(t, folder)

	if _, err := os.Stat(filepath.Join(folder, storage.JsonDataFile)); err != nil {
		t.Fatalf("expected the JSON data file to be created: %v", err)
	}

	engine.WriteEntity("book", map[string]any{"id": "b1", "title": "Dune"})
	engine.WriteEntity("book", map[string]any{"id": "b2", "title": "Anathem"})
	if err := storage.PutKeyValue("greeting", []byte("hello")); err != nil {
		t.Fatalf("kv put: %v", err)
	}

	if _, err := os.Stat(filepath.Join(folder, recovery.RecoveryFile)); err != nil {
		t.Fatalf("expected the JSON recovery log to be active: %v", err)
	}

	if len(storage.GetByWildcardKey("api:entity:book:internal:index:*")) == 0 {
		t.Fatalf("expected the internal engine to keep its indexes in the KV store")
	}
}

func recoverAfterCrash(t *testing.T, folder string) {
	bootInternal(t, folder)

	if got := engine.ReadEntityById("book", "b1"); got == nil || got["title"] != "Dune" {
		t.Fatalf("expected b1 to be recovered after restart, got %v", got)
	}

	sorted := engine.ListEntities("book", 0, 0, "title", true, nil, "", "")
	if len(sorted) != 2 || sorted[0]["id"] != "b2" || sorted[1]["id"] != "b1" {
		t.Fatalf("unexpected sorted list after restart: %v", sorted)
	}

	if v, err := storage.GetByKey("greeting"); err != nil || string(v) != "hello" {
		t.Fatalf("expected the KV value to be recovered, got %q (%v)", v, err)
	}

	if types := api_storage.ListEntityTypes(); len(types) != 1 || types[0] != "book" {
		t.Fatalf("unexpected entity types after restart: %v", types)
	}
}
