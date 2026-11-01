package boot

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transaction"
)

func MonitoringSources() metrics.Sources {
	sources := metrics.Sources{
		DataFolder:      func() string { return globals.GetConfig().Store.Folder },
		KVKeys:          storage.CountKeys,
		EntityCounts:    engine.CountEntitiesPerType,
		IndexBacklog:    api_storage.DirtyFieldsCount,
		RecoverySizes:   recovery.LogSizes,
		Transactions:    transaction.OpenTransactions,
		HTTPConnections: HTTPConnections,
		CacheEntries: func() int {
			if cache.CacheStore == nil {
				return 0
			}
			return cache.CacheStore.Entries()
		},
	}

	if security.UserAuthenticationIsEnabled() {
		sources.AdminSessions = security.ActiveSessionCount
	}

	if security.BasicAuthenticationIsEnabled() || security.UserAuthenticationIsEnabled() {
		sources.DefaultAdminPassword = security.DefaultAdminPasswordIsInUse
	}

	if engine.IsEngineMongoDB() {
		sources.MongoPing = MongoPing
		sources.MongoPoolInUse = func() int { return int(mongoPoolInUse.Load()) }
	}

	return sources
}

func BootMonitoring() {
	metrics.SetSources(MonitoringSources())
	metrics.Start()
}

func WarnIfAuthenticationIsOffOnPublicAddress() {
	cfg := globals.GetConfig()

	hosts := []string{}
	if cfg.Server.HTTP.Enabled {
		hosts = append(hosts, cfg.Server.HTTP.Host)
	}
	if cfg.Server.TCP.Enabled {
		hosts = append(hosts, cfg.Server.TCP.Host)
	}

	if metrics.AuthenticationOffOnPublicAddress(cfg.Security.Authentication.Enabled, hosts) {
		bootLog.Warn("authentication is disabled on a non-loopback address",
			"http", listener(cfg.Server.HTTP),
			"tcp", listener(cfg.Server.TCP),
			"hint", "anyone who can reach these addresses can read and change all the data: enable security.authentication",
		)
	}
}
