package cmd

import (
	"bufio"
	"os"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
)

func ResetAll() {
	Printf("%s\n", "Are you sure you want to reset the database ? This action cannot be undone.")
	Printf("%s\n", "Make sure ElysianDB is down before proceeding.")
	Printf("%s\n", "(yes/no)")

	reader := bufio.NewReader(os.Stdin)
	response, _ := reader.ReadString('\n')
	response = strings.TrimSpace(response)

	if response != "yes" {
		Printf("%s\n", "Reset aborted.")
		return
	}

	force := false
	for _, arg := range os.Args {
		if arg == "--force" {
			force = true
			break
		}
	}

	if !force {
		Printf("%s\n", "Reset requires --force flag to proceed.")
		os.Exit(1)
	}

	cfg := globals.GetConfig()
	dir := cfg.Store.Folder

	os.RemoveAll(dir)
}
