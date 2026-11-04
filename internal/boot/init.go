package boot

import (
	"fmt"
	"path/filepath"
	"time"

	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
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

const migrationGuide = "docs/migration-pebble.md"

func RequireInternalEngine(cfg *configuration.Config) {
	switch cfg.Engine.Name {
	case "", "internal":
	case "mongodb":
		log.Fatal("the mongodb engine was removed", "component", "boot", "engine", cfg.Engine.Name,
			"hint", "set engine.name to internal and restore a backup taken with the previous version, see "+migrationGuide)
	default:
		log.Fatal("unsupported storage engine", "component", "boot", "engine", cfg.Engine.Name, "hint", "use internal, see "+migrationGuide)
	}
}

func InitDB() {
	cfg := globals.GetConfig()
	RequireInternalEngine(cfg)

	if cfg.Api.Cache.Enabled {
		cache.InitCache(time.Duration(cfg.Api.Cache.CleanupIntervalSeconds) * time.Second)
	}

	start := time.Now()
	if err := storage.Open(storage.ModeServer); err != nil {
		log.Fatal("cannot open the data folder", "component", "storage", "folder", cfg.Store.Folder, "err", err)
	}

	logStorageLoaded(cfg, start)
	logStorageSettings(cfg)

	BootACL()
	BootHooks()
	BootApiCacheCleaner()
}

func InitTool() {
	cfg := globals.GetConfig()
	RequireInternalEngine(cfg)

	start := time.Now()
	if err := storage.Open(storage.ModeTool); err != nil {
		log.Fatal("cannot open the data folder", "component", "storage", "folder", cfg.Store.Folder, "err", err,
			"hint", "stop the server first, or use GET /api/backup on the running server")
	}

	logStorageLoaded(cfg, start)
}

func durabilityName(d datastore.Durability) string {
	switch d {
	case datastore.DurabilitySync:
		return "fsync"
	case datastore.DurabilityWrite:
		return "write"
	}

	return "background"
}

func logStorageSettings(cfg *configuration.Config) {
	e := storage.Engine()
	if e == nil {
		return
	}

	opts := e.Settings()
	storageLog.Info("storage settings",
		"engine", "pebble",
		"folder", filepath.Join(cfg.Store.Folder, datastore.PebbleFolder),
		"durability", durabilityName(opts.Durability),
		"wal_sync_interval_s", cfg.Store.FlushIntervalSeconds,
		"memtable_mb", opts.MemTableBytes>>20,
		"block_cache_mb", opts.CacheBytes>>20,
		"kv_lock_shards", cfg.Store.Shards,
		"index_workers", opts.IndexWorkers,
	)

	if cfg.Engine.URI != "" {
		storageLog.Info("configuration key has no effect", "key", "engine.uri", "hint", "the internal engine does not use a connection URI")
	}
}

func logStorageLoaded(cfg *configuration.Config, start time.Time) {
	keys, withTTL := storage.CountKeys()
	storageLog.Info("storage loaded",
		"folder", cfg.Store.Folder,
		"kv_keys", keys,
		"kv_keys_with_ttl", withTTL,
		"documents", storage.CountJsonDocuments(),
		"duration_ms", log.Since(start),
	)
}
