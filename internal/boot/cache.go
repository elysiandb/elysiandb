package boot

import (
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
)

var cacheCleaner sync.Once

func BootApiCacheCleaner() {
	if globals.GetConfig().Api.Cache.Enabled {
		cacheCleaner.Do(func() { go checkApiCachePeriodically() })
	}
}

func checkApiCachePeriodically() {
	for {
		cache.CleanExpiredEntries()
		time.Sleep(1 * time.Second)
	}
}
