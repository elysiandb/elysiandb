package boot

import (
	"fmt"
	"time"

	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/storage"
)

func InitDB() {
	cfg := globals.GetConfig()

	if cfg.Api.Cache.Enabled {
		cache.InitCache(time.Duration(cfg.Api.Cache.CleanupIntervalSeconds) * time.Second)
	}

	storage.LoadDB()
	storage.LoadJsonDB()

	if cfg.Store.CrashRecovery.Enabled {
		recovery.ReplayJsonRecoveryLog(storage.PutJsonValue, storage.DeleteJsonByKey)
		recovery.ActivateJsonRecoveryLog(storage.WriteJsonDB)

		recovery.ReplayStoreRecoveryLog(storage.PutKeyValueWithTTL, storage.DeleteByKey)
		recovery.ActivateStoreRecoveryLog(storage.WriteStoreDB)
	}

	BootSaver()
	BootExpirationHandler()
	BootLazyIndexRebuilder()

	if engine.IsEngineMongoDB() {
		fmt.Printf("%sStorage engine: %s%s\n", globals.Gold, "MongoDB", globals.Reset)
		InitMongoDBConnection()
	} else if engine.IsEngineInternal() {
		fmt.Printf("%sStorage engine: %s%s\n", globals.Gold, "Internal", globals.Reset)
	} else {
		panic("Unsupported engine: " + cfg.Engine.Name)
	}

	BootACL()
	BootHooks()
	BootApiCacheCleaner()
}
