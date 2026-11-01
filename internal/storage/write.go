package storage

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"os"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/recovery"
)

var (
	storeWriteMu  sync.Mutex
	jsonWriteMu   sync.Mutex
	storageLog    = log.For("storage")
	flushThrottle = log.NewThrottle(time.Minute)
	flushFailures sync.Map
)

const (
	flushTargetStore = "store"
	flushTargetJSON  = "json"
)

func WriteToDB() {
	WriteStoreDB()
	WriteJsonDB()
}

func WriteStoreDB() {
	storeWriteMu.Lock()
	defer storeWriteMu.Unlock()

	cfg := globals.GetConfig()

	rootMu.RLock()
	ms := mainStore
	ec := expirationContainer
	rootMu.RUnlock()

	start := time.Now()
	wroteStore, err := writeStoreToFile(cfg, DataFile, ms)
	wroteExpirations := false
	if err == nil {
		wroteExpirations, err = writeExpirationsToFile(cfg, ExpirationDataFile, ec)
	}

	if !finishFlush(flushTargetStore, start, wroteStore || wroteExpirations, err) {
		return
	}

	if cfg.Store.CrashRecovery.Enabled {
		recovery.ClearStoreRecoveryLog()
	}
}

func WriteJsonDB() {
	jsonWriteMu.Lock()
	defer jsonWriteMu.Unlock()

	cfg := globals.GetConfig()

	rootMu.RLock()
	js := mainJsonStore.Load()
	rootMu.RUnlock()

	if js == nil {
		return
	}

	start := time.Now()
	wrote, err := writeJsonStoreToFile(cfg, JsonDataFile, js)
	if !finishFlush(flushTargetJSON, start, wrote, err) {
		return
	}

	if cfg.Store.CrashRecovery.Enabled {
		recovery.ClearJsonRecoveryLog()
	}
}

func finishFlush(target string, start time.Time, wrote bool, err error) bool {
	d := time.Since(start)
	metrics.RecordFlush(target, d, wrote, err)

	counter, _ := flushFailures.LoadOrStore(target, new(atomic.Int64))
	failures := counter.(*atomic.Int64)

	if err != nil {
		failures.Add(1)
		if suppressed, ok := flushThrottle.Allow(target); ok {
			storageLog.Error("flush failed", log.WithSuppressed([]any{"target", target, "err", err}, suppressed)...)
		}
		return false
	}

	if n := failures.Swap(0); n > 0 {
		flushThrottle.Reset(target)
		storageLog.Info("flush working again", "target", target, "failures", n)
	}

	if wrote && log.Enabled(slog.LevelDebug) {
		storageLog.Debug("flush done", "target", target, "duration_ms", log.Since(start))
	}

	return true
}

func writeDataFile(path string, value any) error {
	file, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o600)
	if err != nil {
		return err
	}

	_ = file.Chmod(0o600)

	if err := json.NewEncoder(file).Encode(value); err != nil {
		_ = file.Close()
		return fmt.Errorf("encode %s: %w", path, err)
	}

	if err := file.Close(); err != nil {
		return fmt.Errorf("close %s: %w", path, err)
	}

	return nil
}

func writeExpirationsToFile(cfg *configuration.Config, fileName string, expirationContainer *ExpirationContainer) (bool, error) {
	if expirationContainer.saved.Load() {
		return false, nil
	}

	err := writeDataFile(cfg.Store.Folder+"/"+fileName, expirationContainer.ToMap())
	expirationContainer.saved.Store(err == nil)

	return true, err
}

func writeStoreToFile(cfg *configuration.Config, fileName string, store *Store) (bool, error) {
	if store.saved.Load() {
		return false, nil
	}

	err := writeDataFile(cfg.Store.Folder+"/"+fileName, store.ToMap())
	store.saved.Store(err == nil)

	return true, err
}
