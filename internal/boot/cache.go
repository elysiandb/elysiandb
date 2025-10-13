package boot

import (
	"time"

	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
)

func BootApiCacheCleaner() {
	if globals.GetConfig().Api.Cache.Enabled {
		go checkApiCachePeriodically()
	}
}

func checkApiCachePeriodically() {
	for {
		cache.CacheStore.CleanExpired()
		time.Sleep(1 * time.Second)
	}
}
