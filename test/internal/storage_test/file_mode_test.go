package storage_test

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func expectOwnerOnly(t *testing.T, dir string) {
	t.Helper()

	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}

	if len(entries) == 0 {
		t.Fatalf("expected data files in %s", dir)
	}

	for _, entry := range entries {
		info, err := entry.Info()
		if err != nil {
			t.Fatal(err)
		}

		if info.Mode().IsRegular() && info.Mode().Perm() != 0o600 {
			t.Fatalf("%s: expected mode 0600, got %v", entry.Name(), info.Mode().Perm())
		}
	}
}

func TestDataFilesAreOwnerOnly(t *testing.T) {
	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder:        dir,
			Shards:        4,
			CrashRecovery: configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 1},
		},
	})

	storage.LoadDB()
	storage.LoadJsonDB()
	expectOwnerOnly(t, dir)

	storage.PutKeyValue("k", []byte("v"))
	if err := storage.PutJsonValue("doc", map[string]any{"id": "doc"}); err != nil {
		t.Fatal(err)
	}
	storage.WriteToDB()
	expectOwnerOnly(t, dir)
}

func TestExistingDataFilesBecomeOwnerOnlyAtStartup(t *testing.T) {
	dir := t.TempDir()
	for _, name := range []string{storage.DataFile, storage.ExpirationDataFile, storage.JsonDataFile} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("{}"), 0o644); err != nil {
			t.Fatal(err)
		}
		if err := os.Chmod(filepath.Join(dir, name), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: dir, Shards: 4}})
	storage.LoadDB()
	storage.LoadJsonDB()

	expectOwnerOnly(t, dir)
}
