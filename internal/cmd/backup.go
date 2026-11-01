package cmd

import (
	"errors"
	"flag"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/globals"
)

var Exit = os.Exit

func Backup() {
	path, err := RunBackup(commandArgs())
	if err != nil {
		Printf("%sBackup failed: %v%s\n", globals.Red, err, globals.Reset)
		Exit(1)
		return
	}

	Printf("%sBackup written to %s%s\n", globals.Gold, path, globals.Reset)
	Printf("%sThe archive holds the users key and the password hashes: store it as safely as the data folder.%s\n", globals.Gray, globals.Reset)
}

func RunBackup(args []string) (string, error) {
	output := "."
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
			return "", fmt.Errorf("unknown argument %q, usage: elysiandb backup [--output <folder>]", arg)
		}
	}

	info, err := os.Stat(output)
	if err != nil {
		return "", err
	}

	if !info.IsDir() {
		return "", fmt.Errorf("%s is not a folder", output)
	}

	path, _, err := backup.WriteFile(output, time.Now().UTC())

	return path, err
}

func commandArgs() []string {
	args := flag.Args()
	if len(args) == 0 {
		return nil
	}

	return args[1:]
}
