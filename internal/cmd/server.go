package cmd

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func StartServer() {
	cfg := globals.GetConfig()

	fmt.Printf(
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

	boot.InitDB()

	fmt.Printf(
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
		fmt.Printf(
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
		fmt.Printf(
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
		fmt.Printf(
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
	fmt.Printf(
		"\n%sPersisted%s  %sAll data flushed to disk.%s\n",
		globals.Gold,
		globals.Reset,
		globals.Gray,
		globals.Reset,
	)
	fmt.Printf(
		"%sGoodbye%s   %sElysianDB shutting down gracefully.%s\n",
		globals.Blue,
		globals.Reset,
		globals.Faint,
		globals.Reset,
	)
}
