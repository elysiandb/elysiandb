package cmd

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

var serverLog = log.For("boot")

const mongoDisconnectTimeout = 5 * time.Second

func StartServer() {
	cfg := globals.GetConfig()

	if cfg.Stats.Enabled && engine.IsEngineInternal() {
		boot.BootStats()
	}

	boot.WarnIfAuthenticationIsOffOnPublicAddress()
	security.WarnIfDefaultAdminPassword()

	if cfg.Server.HTTP.Enabled {
		go boot.StartHTTP()
	}

	if cfg.Server.TCP.Enabled {
		go boot.InitTCP()
	}

	boot.BootMonitoring()

	if cfg.AdminUI.Enabled && cfg.Server.HTTP.Enabled {
		serverLog.Info("admin ui available", "url", fmt.Sprintf("http://%s:%d/admin", cfg.Server.HTTP.Host, cfg.Server.HTTP.Port))
	}

	signals := make(chan os.Signal, 1)
	signal.Notify(signals, os.Interrupt, syscall.SIGTERM)
	received := <-signals
	signal.Stop(signals)

	serverLog.Info("shutdown requested", "signal", received.String())

	start := time.Now()
	storage.WriteToDB()
	serverLog.Info("data flushed on shutdown", "duration_ms", log.Since(start))

	if globals.MongoClient != nil {
		ctx, cancel := context.WithTimeout(context.Background(), mongoDisconnectTimeout)
		if err := globals.MongoClient.Disconnect(ctx); err != nil {
			log.Warn("mongodb disconnect failed", "component", "mongodb", "err", err)
		}
		cancel()
	}

	metrics.Stop()

	serverLog.Info("shutdown complete", "uptime", time.Since(metrics.StartedAt()).Round(time.Second).String())
}
