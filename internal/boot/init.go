package boot

import (
	"time"

	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func InitDB() {
	cfg := globals.GetConfig()

	if cfg.ApiCache.Enabled {
		cache.InitCache(time.Duration(cfg.ApiCache.CleanupIntervalSeconds) * time.Second)
	}

	storage.LoadDB()
	storage.LoadJsonDB()
	BootSaver()
	BootExpirationHandler()
	BootLogger()
}
