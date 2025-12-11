package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/taymour/elysiandb/internal/cmd"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
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
	banner()

	configFilename := flag.String("config", "elysian.yaml", "Path to configuration file")
	flag.Parse()

	cfg, err := configuration.LoadConfig(*configFilename)
	if err != nil {
		log.DirectError("Error loading config:", err)
		return
	}

	globals.SetConfig(cfg)

	args := os.Args

	if len(args) == 1 {
		cmd.StartServer()
		return
	}

	handlers := cmd.GetHandlers()

	if handler, ok := handlers[args[1]]; ok {
		handler()
	} else {
		fmt.Printf("%sUnknown command: %s%s\n", globals.Gold, args[1], globals.Reset)
		printListOfCommands()
	}
}

func printListOfCommands() {
	commands := cmd.GetAvailableCommands()

	for name, description := range commands {
		fmt.Printf("  %s%s%s  %s\n", globals.Bold, name, globals.Reset, description)
	}
}
