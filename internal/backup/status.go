package backup

import (
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/log"
)

var backupLog = log.For("backup")

type Status struct {
	Running   int64
	Completed int64
	Failed    int64
	LastAt    time.Time
	LastOK    bool
	Duration  time.Duration
	Bytes     int64
	Documents int64
	LastError string
}

var (
	statusMu sync.Mutex
	status   Status
	running  atomic.Int64
)

func CurrentStatus() Status {
	statusMu.Lock()
	defer statusMu.Unlock()

	out := status
	out.Running = running.Load()

	return out
}

func begin() func(*Manifest, int64, error) {
	running.Add(1)
	start := time.Now()

	return func(m *Manifest, bytes int64, err error) {
		running.Add(-1)

		documents := int64(0)
		for _, e := range m.Entities {
			documents += int64(e.Documents)
		}

		statusMu.Lock()
		status.LastAt = time.Now()
		status.LastOK = err == nil
		status.Duration = time.Since(start)
		status.Bytes = bytes
		status.Documents = documents
		if err == nil {
			status.Completed++
			status.LastError = ""
		} else {
			status.Failed++
			status.LastError = err.Error()
		}
		statusMu.Unlock()

		if err != nil {
			backupLog.Error("backup failed", "err", err, "duration_ms", log.Since(start))
			return
		}

		backupLog.Info("backup written", "documents", documents, "bytes", bytes, "duration_ms", log.Since(start))
	}
}
