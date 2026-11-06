package storage_test

import (
	"io/fs"
	"os"
	"path/filepath"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func expectOwnerOnly(t *testing.T, dir string) {
	t.Helper()

	files := 0
	err := filepath.WalkDir(dir, func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}

		info, err := d.Info()
		if err != nil {
			return err
		}

		files++
		if info.Mode().IsRegular() && info.Mode().Perm() != 0o600 {
			t.Fatalf("%s: expected mode 0600, got %v", path, info.Mode().Perm())
		}

		return nil
	})
	if err != nil {
		t.Fatal(err)
	}

	if files == 0 {
		t.Fatalf("expected data files in %s", dir)
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
	t.Cleanup(func() { _ = storage.Close() })
	expectOwnerOnly(t, dir)

	storage.PutKeyValue("k", []byte("v"))
	if err := storage.PutJsonValue("api:entity:docs:id:doc", map[string]any{"id": "doc"}); err != nil {
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
	t.Cleanup(func() { _ = storage.Close() })

	expectOwnerOnly(t, dir)
}
