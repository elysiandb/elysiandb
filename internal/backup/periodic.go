package backup

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/durable"
	"github.com/taymour/elysiandb/internal/globals"
)

const defaultKeptBackups = 7

func StartPeriodic() func() {
	cfg := globals.GetConfig().Store.Backup
	if cfg.EveryMinutes <= 0 || cfg.Folder == "" {
		return func() {}
	}

	stop := make(chan struct{})
	go func() {
		ticker := time.NewTicker(time.Duration(cfg.EveryMinutes) * time.Minute)
		defer ticker.Stop()

		for {
			select {
			case <-stop:
				return
			case <-ticker.C:
				RunPeriodic(cfg.Folder, cfg.Keep)
			}
		}
	}()

	return func() { close(stop) }
}

func RunPeriodic(folder string, keep int) {
	if err := os.MkdirAll(folder, 0o700); err != nil {
		backupLog.Error("cannot create the backup folder", "folder", folder, "err", err)
		return
	}

	durable.RunGently(func() {
		if _, _, err := writeFile(folder, time.Now().UTC(), Options{Pace: true}); err != nil {
			backupLog.Error("periodic backup failed", "folder", folder, "err", err)
			return
		}

		pruneBackups(folder, keep)
	})
}

func pruneBackups(folder string, keep int) {
	if keep <= 0 {
		keep = defaultKeptBackups
	}

	entries, err := os.ReadDir(folder)
	if err != nil {
		return
	}

	names := []string{}
	for _, entry := range entries {
		name := entry.Name()
		if strings.HasPrefix(name, fileNamePrefix) && strings.HasSuffix(name, FileNameExtension) {
			names = append(names, name)
		}
	}

	sort.Strings(names)
	for i := 0; i < len(names)-keep; i++ {
		if err := os.Remove(filepath.Join(folder, names[i])); err != nil {
			backupLog.Warn("cannot remove an old backup", "file", names[i], "err", err)
		}
	}
}
