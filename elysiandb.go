package main

import (
	"context"
	"flag"
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

const (
	reset  = "\033[0m"
	bold   = "\033[1m"
	faint  = "\033[2m"
	gold   = "\033[38;5;220m"
	blue   = "\033[38;5;27m"
	violet = "\033[38;5;57m"
	gray   = "\033[38;5;245m"
)

func banner() {
	fmt.Printf("\n%s", blue)
	fmt.Println(" ╔═══════════════════════════════════════════════════════════════════╗")
	fmt.Printf(" ║ %s%-63s%s   ║\n", gold+bold, "ElysianDB", reset+blue)
	fmt.Printf(" ║ %s%-63s%s   ║\n", gray, "A modern, lightweight KV datastore", reset+blue)
	fmt.Printf(" ║ %s%-63s%s   ║\n", gold, "→ Instant REST API, out of the box", reset+blue)
	fmt.Println(" ╚═══════════════════════════════════════════════════════════════════╝" + reset)
}

func main() {
	banner()

	configFilename := flag.String("config", "elysian.yaml", "Path to configuration file")
	flag.Parse()

	cfg, err := configuration.LoadConfig(*configFilename)
	if err != nil {
		log.Error("Error loading config:", err)
		return
	}
	globals.SetConfig(cfg)

	fmt.Printf("%s%sStorage%s  %s%s%s\n", violet, bold, reset, gray, globals.GetConfig().Store.Folder, reset)

	if cfg.Stats.Enabled {
		boot.BootStats()
	}

	boot.InitDB()

	fmt.Printf("%s%sReady%s   %sServing a KV datastore with %sInstant REST API%s.\n", blue, bold, reset, gray, gold, reset)

	if cfg.Server.HTTP.Enabled {
		go boot.StartHTTP()
		fmt.Printf("%sHTTP%s     %shttp://%s:%d%s  %s(KV store & Instant REST API)%s\n",
			gold, reset, bold, cfg.Server.HTTP.Host, cfg.Server.HTTP.Port, reset, gray, reset)
	}
	if cfg.Server.TCP.Enabled {
		go boot.InitTCP()
		fmt.Printf("%sTCP %s     %s%s:%d%s\n", gold, reset, bold, cfg.Server.TCP.Host, cfg.Server.TCP.Port, reset)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	<-ctx.Done()

	storage.WriteToDB()
	fmt.Printf("\n%sPersisted%s  %sAll data flushed to disk.%s\n", gold, reset, gray, reset)
	fmt.Printf("%sGoodbye%s   %sElysianDB shutting down gracefully.%s\n", blue, reset, faint, reset)
}
