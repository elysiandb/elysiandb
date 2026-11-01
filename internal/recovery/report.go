package recovery

import (
	"os"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
)

var (
	recoveryLog   = log.For("recovery")
	writeThrottle = log.NewThrottle(time.Minute)
)

func reportWriteFailure(path string, err error) {
	metrics.RecoveryWriteFailed(err)

	if suppressed, ok := writeThrottle.Allow(path); ok {
		recoveryLog.Error("recovery log write failed", log.WithSuppressed([]any{"file", path, "err", err}, suppressed)...)
	}
}

func reportReplay(name, path string, entries, unreadable int, start time.Time) {
	if unreadable > 0 {
		recoveryLog.Error("recovery log entries unreadable", "log", name, "file", path, "entries", unreadable)
	}

	if entries == 0 {
		return
	}

	metrics.RecoveryReplayed(entries)
	recoveryLog.Warn("unclean shutdown detected, recovery log replayed", "log", name, "entries", entries, "duration_ms", log.Since(start))
}

func reportForcedFlush(name string, size int64) {
	metrics.ForcedFlush()
	recoveryLog.Debug("recovery log size limit reached, flushing", "log", name, "bytes", size)
}

func LogSizes() (int64, int64) {
	cfg := globals.GetConfig()

	return fileSize(cfg.Store.Folder + "/" + RecoveryFile), fileSize(cfg.Store.Folder + "/" + StoreRecoveryFile)
}

func fileSize(path string) int64 {
	info, err := os.Stat(path)
	if err != nil {
		return 0
	}

	return info.Size()
}
