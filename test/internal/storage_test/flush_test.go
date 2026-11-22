package storage_test

import (
	"log/slog"
	"os"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/storage"
)

func TestFailedCheckpointIsReportedAndRetried(t *testing.T) {
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
		_ = os.Chmod(dir, 0o700)
		_ = storage.Close()
		log.SetOutput(nil, false)
		_ = log.Configure(log.Options{})
		metrics.Reset()
	})

	storage.LoadDB()
	if err := storage.PutKeyValue("k", []byte("v")); err != nil {
		t.Fatal(err)
	}

	if err := os.Chmod(dir, 0o500); err != nil {
		t.Fatal(err)
	}

	if err := storage.Checkpoint(); err == nil {
		t.Fatalf("a checkpoint in a read-only folder must fail")
	}

	failures := log.Logs(log.Query{MinLevel: slog.LevelError}).Entries
	if len(failures) != 1 || failures[0].Message != "flush failed" || failures[0].Component != "storage" || failures[0].Attrs["target"] != "store" {
		t.Fatalf("expected one flush failed line, got %+v", failures)
	}
	if p := metrics.Persistence(); p.LastFlushOK || p.FlushFailures != 1 {
		t.Fatalf("the failure must be visible in the metrics: %+v", p)
	}

	_ = storage.Checkpoint()
	if got := log.Logs(log.Query{MinLevel: slog.LevelError}).Entries; len(got) != 1 {
		t.Fatalf("repeated failures are throttled, got %d lines", len(got))
	}

	if err := os.Chmod(dir, 0o700); err != nil {
		t.Fatal(err)
	}

	if err := storage.Checkpoint(); err != nil {
		t.Fatal(err)
	}

	recovered := log.Logs(log.Query{Text: "flush working again"}).Entries
	if len(recovered) != 1 || recovered[0].Attrs["failures"] != int64(2) {
		t.Fatalf("expected a recovery line with 2 failures, got %+v", recovered)
	}

	if p := metrics.Persistence(); !p.LastFlushOK || p.LastSuccessAt == nil {
		t.Fatalf("persistence should be ok again: %+v", p)
	}

	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}

	storage.LoadDB()
	if v, err := storage.GetByKey("k"); err != nil || string(v) != "v" {
		t.Fatalf("the key survives a restart: %q %v", v, err)
	}
}
