package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func main() {
	out := flag.String("out", "", "output folder tests/golden/<part>")
	part := flag.String("part", "", "part name")
	flag.Parse()
	if *out == "" || *part == "" {
		fmt.Fprintln(os.Stderr, "missing -out or -part")
		os.Exit(2)
	}
	if err := goldenio.WriteManifest(*out, *part); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
