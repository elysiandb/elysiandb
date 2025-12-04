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
		log.Error("Error loading config:", err)
		return
	}
	globals.SetConfig(cfg)

	args := os.Args

	if len(args) == 1 {
		cmd.StartServer()
		return
	}

	switch args[1] {
	case "server":
		cmd.StartServer()
	case "create-user":
		cmd.CreateUser()
	default:
		fmt.Printf("%sUnknown command: %s%s\n", globals.Gold, args[1], globals.Reset)
		printListOfCommands()
	}
}

func printListOfCommands() {
	fmt.Printf("%sAvailable commands:%s\n", globals.Gold, globals.Reset)
	fmt.Printf("  %sserver%s       Start ElysianDB server\n", globals.Bold, globals.Reset)
	fmt.Printf("  %screate-user%s  Create a new user\n", globals.Bold, globals.Reset)
}
