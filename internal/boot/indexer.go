package boot

import (
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
)

func BootLazyIndexRebuilder() {
	api_storage.RebuildAllIndexes()

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
