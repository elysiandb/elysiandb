package cmd

import (
	"bufio"
	"context"
	"os"
	"strings"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/mongodb"
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

	if engine.IsEngineMongoDB() {
		boot.InitMongoDBConnection()
		defer globals.MongoClient.Disconnect(context.Background())

		types := engine.ListEntityTypes()
		for _, t := range types {
			err := mongodb.DeleteEntityType(t)
			if err != nil {
				Printf("Failed to delete entity type '%s': %v\n", t, err)
			}
		}
	}
}
