package boot

import (
	"fmt"
	"time"

	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/storage"
)

var (
	bootLog    = log.For("boot")
	storageLog = log.For("storage")
)

func LogConfiguration(path string) {
	cfg := globals.GetConfig()

	bootLog.Info("configuration loaded",
		"path", path,
		"engine", globals.GetEngine(),
		"auth_mode", metrics.AuthMode(),
		"http", listener(cfg.Server.HTTP),
		"tcp", listener(cfg.Server.TCP),
		"admin_ui", cfg.AdminUI.Enabled,
		"crash_recovery", cfg.Store.CrashRecovery.Enabled,
		"cache", cfg.Api.Cache.Enabled,
		"hooks", cfg.Api.Hooks.Enabled,
		"schema", cfg.Api.Schema.Enabled,
		"stats", cfg.Stats.Enabled,
		"log_level", log.LevelName(log.ConfiguredLevel()),
		"log_format", log.Format(),
	)

	if cfg.Log.FlushIntervalSeconds != 0 {
		bootLog.Warn("deprecated configuration key is ignored", "key", "log.flushIntervalSeconds", "hint", "logs are now written immediately, remove the key")
	}
}

func listener(server configuration.ServerConfig) string {
	if !server.Enabled {
		return "disabled"
	}

	return fmt.Sprintf("%s:%d", server.Host, server.Port)
}

func InitDB() {
	cfg := globals.GetConfig()

	if cfg.Api.Cache.Enabled {
		cache.InitCache(time.Duration(cfg.Api.Cache.CleanupIntervalSeconds) * time.Second)
	}

	start := time.Now()
	storage.LoadDB()
	storage.LoadJsonDB()

	if cfg.Store.CrashRecovery.Enabled {
		recovery.ReplayJsonRecoveryLog(storage.PutJsonValue, storage.DeleteJsonByKey)
		recovery.ActivateJsonRecoveryLog(storage.WriteJsonDB)

		recovery.ReplayStoreRecoveryLog(storage.PutKeyValueWithExpiration, storage.DeleteByKey)
		recovery.ActivateStoreRecoveryLog(storage.WriteStoreDB)
	}

	keys, withTTL := storage.CountKeys()
	storageLog.Info("storage loaded",
		"folder", cfg.Store.Folder,
		"kv_keys", keys,
		"kv_keys_with_ttl", withTTL,
		"documents", storage.CountJsonDocuments(),
		"duration_ms", log.Since(start),
	)

	BootSaver()
	BootExpirationHandler()
	BootLazyIndexRebuilder()

	switch {
	case engine.IsEngineMongoDB():
		InitMongoDBConnection()
	case engine.IsEngineInternal():
	default:
		log.Fatal("unsupported storage engine", "component", "boot", "engine", cfg.Engine.Name, "hint", "use internal or mongodb")
	}

	BootACL()
	BootHooks()
	BootApiCacheCleaner()
}
