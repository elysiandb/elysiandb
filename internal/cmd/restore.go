package cmd

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"maps"
	"os"
	"slices"
	"strings"

	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/globals"
)

var ErrRestoreAborted = errors.New("restore aborted")

func Restore() {
	report, err := RunRestore(commandArgs(), os.Stdin)
	if errors.Is(err, ErrRestoreAborted) {
		Printf("%s\n", "Restore aborted.")
		return
	}

	if err != nil {
		Printf("%sRestore failed: %v%s\n", globals.Red, err, globals.Reset)
		Exit(1)
		return
	}

	Printf("%sRestore completed.%s\n", globals.Gold, globals.Reset)
	for _, entity := range slices.Sorted(maps.Keys(report.Entities)) {
		Printf("  %s: %d documents\n", entity, report.Entities[entity])
	}

	Printf("  schemas: %d\n", report.Schemas)
	Printf("  KV keys: %d (%d expired and skipped)\n", report.KVKeys, report.ExpiredKVKeys)
}

func RunRestore(args []string, in io.Reader) (*backup.Report, error) {
	file := ""
	force := false
	for _, arg := range args {
		switch {
		case arg == "--force" || arg == "-force":
			force = true
		case strings.HasPrefix(arg, "-"):
			return nil, fmt.Errorf("unknown argument %q, usage: elysiandb restore <file> [--force]", arg)
		case file == "":
			file = arg
		default:
			return nil, errors.New("usage: elysiandb restore <file> [--force]")
		}
	}

	if file == "" {
		return nil, errors.New("usage: elysiandb restore <file> [--force]")
	}

	f, err := os.Open(file)
	if err != nil {
		return nil, err
	}

	defer f.Close()

	if _, err := backup.Verify(f); err != nil {
		return nil, err
	}

	if !force && !confirmRestore(in) {
		return nil, ErrRestoreAborted
	}

	if _, err := f.Seek(0, io.SeekStart); err != nil {
		return nil, err
	}

	return backup.Restore(f)
}

func confirmRestore(in io.Reader) bool {
	Printf("%s\n", "The restore replaces all the data, users and KV keys with the content of the archive. This action cannot be undone.")
	Printf("%s\n", "Make sure ElysianDB is down before proceeding.")
	Printf("%s\n", "Type yes to continue:")

	response, _ := bufio.NewReader(in).ReadString('\n')

	return strings.TrimSpace(response) == "yes"
}
