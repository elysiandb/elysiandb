package boot

import (
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
)

func BootLazyIndexRebuilder() {
	start := time.Now()
	api_storage.RebuildAllIndexes()
	log.Debug("indexes rebuilt", "component", "storage", "duration_ms", log.Since(start))

	if globals.GetConfig().Server.HTTP.Enabled {
		for i := 0; i < globals.GetConfig().Api.Index.Workers; i++ {
			go rebuildDirtyIndexesWorker()
		}
	}
}

func rebuildDirtyIndexesWorker() {
	for {
		api_storage.ProcessNextDirtyField()
		time.Sleep(100 * time.Millisecond)
	}
}
