package main

import (
	"flag"
	"fmt"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/cmd"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/security"
)

func banner() {
	fmt.Printf("\n%s", globals.Blue)
	fmt.Println(" ╔═══════════════════════════════════════════════════════════════════╗")
	fmt.Printf(" ║ %s%-63s%s   ║\n", globals.Gold+globals.Bold, "ElysianDB", globals.Reset+globals.Blue)
	fmt.Printf(" ║ %s%-63s%s   ║\n", globals.Gray, "A modern, lightweight KV datastore", globals.Reset+globals.Blue)
	fmt.Printf(" ║ %s%-63s%s   ║\n", globals.Gold, "→ Instant REST API, out of the box", globals.Reset+globals.Blue)
	fmt.Println(" ╚═══════════════════════════════════════════════════════════════════╝" + globals.Reset)
}

func main() {
	configFilename := flag.String("config", "elysian.yaml", "Path to configuration file")
	flag.Parse()

	cfg, err := configuration.LoadConfig(*configFilename)
	if err != nil {
		log.Fatal("cannot load the configuration", "component", "boot", "path", *configFilename, "err", err)
	}

	if err := log.Configure(log.Options{Level: cfg.Log.Level, Format: cfg.Log.Format, BufferLines: cfg.Log.BufferLines}); err != nil {
		log.Fatal("invalid log configuration", "component", "boot", "path", *configFilename, "err", err)
	}

	if log.Format() == log.FormatText && log.StdoutIsTerminal() {
		banner()
	}

	globals.SetConfig(cfg)
	boot.LogConfiguration(*configFilename)

	boot.InitDB()
	if err := security.InitAdminUserIfNotExists(); err != nil {
		log.Error("default admin creation failed", "component", "security", "err", err)
	}
	security.ProtectSessionsFile()

	args := flag.Args()

	if len(args) == 0 {
		cmd.StartServer()
		return
	}

	handlers := cmd.GetHandlers()

	if handler, ok := handlers[args[0]]; ok {
		handler()
	} else {
		fmt.Printf("%sUnknown command: %s%s\n", globals.Gold, args[0], globals.Reset)
		printListOfCommands()
	}
}

func printListOfCommands() {
	cmd.PrintHelp()
}
