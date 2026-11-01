package storage_test

import (
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/storage"
)

func TestFailedFlushKeepsTheRecoveryLogAndIsReported(t *testing.T) {
	if os.Geteuid() == 0 {
		t.Skip("root ignores file permissions")
	}

	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder:        dir,
			Shards:        4,
			CrashRecovery: configuration.CrashRecoveryConfig{Enabled: true, MaxLogMB: 100},
		},
	})

	if err := log.Configure(log.Options{BufferLines: 100}); err != nil {
		t.Fatal(err)
	}
	log.SetOutput(&strings.Builder{}, false)
	metrics.Reset()
	t.Cleanup(func() {
		log.SetOutput(nil, false)
		_ = log.Configure(log.Options{})
		metrics.Reset()
	})

	storage.LoadDB()
	storage.LoadJsonDB()
	recovery.ActivateStoreRecoveryLog(storage.WriteStoreDB)

	if err := storage.PutKeyValue("k", []byte("v")); err != nil {
		t.Fatal(err)
	}

	recoveryPath := filepath.Join(dir, recovery.StoreRecoveryFile)
	dataPath := filepath.Join(dir, storage.DataFile)
	if _, err := os.Stat(recoveryPath); err != nil {
		t.Fatalf("expected a recovery log: %v", err)
	}

	if err := os.Chmod(dataPath, 0o400); err != nil {
		t.Fatal(err)
	}
	storage.WriteStoreDB()

	if _, err := os.Stat(recoveryPath); err != nil {
		t.Fatalf("a failed flush must keep the recovery log: %v", err)
	}

	failures := log.Logs(log.Query{MinLevel: slog.LevelError}).Entries
	if len(failures) != 1 || failures[0].Message != "flush failed" || failures[0].Component != "storage" || failures[0].Attrs["target"] != "store" {
		t.Fatalf("expected one flush failed line, got %+v", failures)
	}
	if p := metrics.Persistence(); p.LastFlushOK || p.FlushFailures != 1 {
		t.Fatalf("the failure must be visible in the metrics: %+v", p)
	}

	storage.WriteStoreDB()
	if got := log.Logs(log.Query{MinLevel: slog.LevelError}).Entries; len(got) != 1 {
		t.Fatalf("repeated failures are throttled, got %d lines", len(got))
	}

	if err := os.Chmod(dataPath, 0o600); err != nil {
		t.Fatal(err)
	}
	storage.WriteStoreDB()

	if _, err := os.Stat(recoveryPath); !os.IsNotExist(err) {
		t.Fatalf("a successful flush clears the recovery log, got %v", err)
	}

	recovered := log.Logs(log.Query{Text: "flush working again"}).Entries
	if len(recovered) != 1 || recovered[0].Attrs["failures"] != int64(2) {
		t.Fatalf("expected a recovery line with 2 failures, got %+v", recovered)
	}

	data, err := os.ReadFile(dataPath)
	if err != nil || !strings.Contains(string(data), `"k"`) {
		t.Fatalf("the key should be on disk: %s %v", data, err)
	}

	if p := metrics.Persistence(); !p.LastFlushOK || p.LastSuccessAt == nil {
		t.Fatalf("persistence should be ok again: %+v", p)
	}
}
