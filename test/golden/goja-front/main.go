package main

import (
	"fmt"
	"math"
	"os"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("goja-front", run)
}

func run(out string) error {
	steps := []func(string) error{
		genBig,
		genURL,
		genSourcemap,
		genRangetable,
		genToken,
		genRegexp,
		genAST,
	}
	for _, step := range steps {
		if err := step(out); err != nil {
			return err
		}
	}
	return nil
}

func fbits(f float64) string {
	return fmt.Sprintf("%016x", math.Float64bits(f))
}
