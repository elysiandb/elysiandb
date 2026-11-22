package cmd

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/legacy"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

func CloseStorage() {
	if err := storage.Close(); err != nil {
		Printf("%sCannot close the data folder cleanly: %v%s\n", globals.Red, err, globals.Reset)
	}
}

func ExportLegacy() {
	folder, err := RunExportLegacy(commandArgs())
	if err != nil {
		Printf("%sExport failed: %v%s\n", globals.Red, err, globals.Reset)
		CloseStorage()
		Exit(1)
		return
	}

	Printf("%sA 0.2.4 data folder was written to %s%s\n", globals.Gold, folder, globals.Reset)
	Printf("%sStart ElysianDB 0.2.4 with store.folder set to it to go back.%s\n", globals.Gray, globals.Reset)
}

func RunExportLegacy(args []string) (string, error) {
	output := ""
	for i := 0; i < len(args); i++ {
		arg := args[i]
		switch {
		case arg == "--output" || arg == "-output" || arg == "-o":
			if i+1 >= len(args) {
				return "", errors.New("--output needs a folder")
			}

			i++
			output = args[i]
		case strings.HasPrefix(arg, "--output="):
			output = strings.TrimPrefix(arg, "--output=")
		default:
			return "", fmt.Errorf("unknown argument %q, usage: elysiandb export-legacy --output <folder>", arg)
		}
	}

	if output == "" {
		return "", errors.New("usage: elysiandb export-legacy --output <folder>")
	}

	if entries, err := os.ReadDir(output); err == nil && len(entries) > 0 {
		return "", fmt.Errorf("%s is not empty", output)
	}

	e := storage.Engine()
	if e == nil {
		return "", errors.New("the data folder is not open")
	}

	if _, err := e.ExportLegacy(output); err != nil {
		return "", err
	}

	for _, name := range []string{security.KeyFilename, security.SessionsFilename} {
		data, err := os.ReadFile(filepath.Join(globals.GetConfig().Store.Folder, name))
		if err != nil {
			if os.IsNotExist(err) {
				continue
			}

			return "", err
		}

		if err := os.WriteFile(filepath.Join(output, name), data, 0o600); err != nil {
			return "", err
		}
	}

	if len(legacy.FilesIn(output)) == 0 {
		return "", errors.New("nothing was written")
	}

	return output, nil
}
