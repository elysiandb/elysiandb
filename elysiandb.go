package main

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/storage"
)

func main() {
	fmt.Println(`
   ╔══════════════════════════════════════╗
   ║                                      ║
   ║      Welcome to ElysianDB            ║
   ║  A modern, lightweight KV datastore  ║
   ║                                      ║
   ╚══════════════════════════════════════╝
	`)

	cfg, err := configuration.LoadConfig("elysian.yaml")
	if err != nil {
		log.Error("Error loading config:", err)
		return
	}

	globals.SetConfig(cfg)

	log.DirectInfo("Using data folder: ", globals.GetConfig().Store.Folder)

	if cfg.Stats.Enabled {
		boot.BootStats()
	}

	boot.InitDB()

	log.DirectInfo("Ready to serve your key-value needs with elegance.")

	if cfg.Server.HTTP.Enabled {
		go boot.StartHTTP()
	}

	if cfg.Server.TCP.Enabled {
		go boot.InitTCP()
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	<-ctx.Done()

	storage.WriteToDB()

	log.DirectInfo("Data persisted successfully.")

	log.DirectInfo("ElysianDB shutting down gracefully. Goodbye!")
}
