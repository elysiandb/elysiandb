package boot

import (
	"time"

	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/storage"
)

func InitDB() {
	cfg := globals.GetConfig()

	if cfg.ApiCache.Enabled {
		cache.InitCache(time.Duration(cfg.ApiCache.CleanupIntervalSeconds) * time.Second)
	}

	storage.LoadDB()
	storage.LoadJsonDB()

	if cfg.Store.CrashRecovery.Enabled {
		recovery.ReplayJsonRecoveryLog(storage.PutJsonValue, storage.DeleteJsonByKey)
		recovery.ActivateJsonRecoveryLog(storage.WriteJsonDB)

		recovery.ReplayStoreRecoveryLog(storage.PutKeyValue, storage.DeleteByKey)
		recovery.ActivateStoreRecoveryLog(storage.WriteStoreDB)
	}

	BootSaver()
	BootExpirationHandler()
	BootLogger()
	BootApiCacheCleaner()
}
