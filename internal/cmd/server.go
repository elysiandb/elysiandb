package cmd

import (
	"context"
	"os"
	"os/signal"
	"syscall"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func StartServer() {
	cfg := globals.GetConfig()

	Printf(
		"%s%sStorage%s  %s%s%s\n",
		globals.Violet,
		globals.Bold,
		globals.Reset,
		globals.Gray,
		globals.GetConfig().Store.Folder,
		globals.Reset,
	)

	if cfg.Stats.Enabled {
		boot.BootStats()
	}

	boot.BootLogger()

	Printf(
		"%s%sReady%s   %sServing a KV datastore with %sInstant REST API%s.\n",
		globals.Blue,
		globals.Bold,
		globals.Reset,
		globals.Gray,
		globals.Gold,
		globals.Reset,
	)

	if cfg.Server.HTTP.Enabled {
		go boot.StartHTTP()
		Printf(
			"%sHTTP%s     %shttp://%s:%d%s  %s(KV store & Instant REST API)%s\n",
			globals.Gold,
			globals.Reset,
			globals.Bold,
			cfg.Server.HTTP.Host,
			cfg.Server.HTTP.Port,
			globals.Reset,
			globals.Gray,
			globals.Reset,
		)
	}
	if cfg.Server.TCP.Enabled {
		go boot.InitTCP()
		Printf(
			"%sTCP %s     %s%s:%d%s\n",
			globals.Gold,
			globals.Reset,
			globals.Bold,
			cfg.Server.TCP.Host,
			cfg.Server.TCP.Port,
			globals.Reset,
		)
	}

	if cfg.AdminUI.Enabled {
		Printf(
			"%sAdmin dashboard%s    %shttp://%s:%d/admin%s\n",
			globals.Gold,
			globals.Reset,
			globals.Bold,
			cfg.Server.HTTP.Host,
			cfg.Server.HTTP.Port,
			globals.Reset,
		)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	<-ctx.Done()

	storage.WriteToDB()
	Printf(
		"\n%sPersisted%s  %sAll data flushed to disk.%s\n",
		globals.Gold,
		globals.Reset,
		globals.Gray,
		globals.Reset,
	)
	Printf(
		"%sGoodbye%s   %sElysianDB shutting down gracefully.%s\n",
		globals.Blue,
		globals.Reset,
		globals.Faint,
		globals.Reset,
	)
}
