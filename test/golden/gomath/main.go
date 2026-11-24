package main

import (
	"flag"
	"fmt"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

const seed = 20261124

var compare = flag.String("compare", "", "folder written by the run on the other architecture: its case files are checked against this corpus and copied")

func main() {
	goldenio.Main("gomath", run)
}

func hex(f float64) string {
	return fmt.Sprintf("%016x", math.Float64bits(f))
}

func cases() string {
	var b strings.Builder
	rng := rand.New(rand.NewSource(seed))
	xs := []float64{0, math.Copysign(0, -1), 1, -1, 0.5, 2, 3, 10, 1e-300, 1e300, 5e-324, 2.2250738585072014e-308, 709.78, 709.79, -745.13, -745.14, 1e-9, -1e-9, 0.6931471805599453, 1.4142135623730951 / 2, 123.456, 1e16, 7.07106781186547524401e-01}
	for i := 0; i < 2000; i++ {
		xs = append(xs, (rng.Float64()-0.5)*1400)
		xs = append(xs, math.Ldexp(0.5+rng.Float64()/2, rng.Intn(2020)-1010))
		xs = append(xs, rng.Float64()*4)
	}
	for _, x := range xs {
		fmt.Fprintf(&b, "exp %s %s\n", hex(x), hex(math.Exp(x)))
		fmt.Fprintf(&b, "log %s %s\n", hex(x), hex(math.Log(x)))
	}
	for i := 0; i < 3000; i++ {
		x := (rng.Float64() - 0.3) * 40
		y := (rng.Float64() - 0.5) * 20
		if i%3 == 0 {
			y = math.Floor(y)
		}
		if i%5 == 0 {
			x = math.Floor(x)
		}
		fmt.Fprintf(&b, "pow %s %s %s\n", hex(x), hex(y), hex(math.Pow(x, y)))
	}
	pairs := [][2]float64{{2, 0.5}, {10, -3}, {1.1, 1e3}, {-8, 1.0 / 3}, {0, -1}, {math.Copysign(0, -1), -3}, {math.Inf(-1), 3}, {1e308, 2}, {2, -1074}, {7, 0.1}, {0.1, 0.1}}
	for _, p := range pairs {
		fmt.Fprintf(&b, "pow %s %s %s\n", hex(p[0]), hex(p[1]), hex(math.Pow(p[0], p[1])))
	}
	return b.String()
}

func inputs(line string) string {
	i := strings.LastIndexByte(line, ' ')
	if i < 0 {
		return line
	}
	return line[:i]
}

func checkCorpus(name, other, own string) error {
	a := strings.Split(strings.TrimSuffix(other, "\n"), "\n")
	b := strings.Split(strings.TrimSuffix(own, "\n"), "\n")
	if len(a) != len(b) {
		return fmt.Errorf("%s: %d lines, %d computed", name, len(a), len(b))
	}
	for i := range a {
		if inputs(a[i]) != inputs(b[i]) {
			return fmt.Errorf("%s:%d: corpus differs between architectures", name, i+1)
		}
	}
	return nil
}

func run(out string) error {
	own := cases()
	name := runtime.GOARCH + ".cases"
	if *compare != "" {
		entries, err := os.ReadDir(*compare)
		if err != nil {
			return err
		}
		copied := 0
		for _, e := range entries {
			if e.IsDir() || !strings.HasSuffix(e.Name(), ".cases") || e.Name() == name {
				continue
			}
			data, err := os.ReadFile(filepath.Join(*compare, e.Name()))
			if err != nil {
				return err
			}
			if err := checkCorpus(e.Name(), string(data), own); err != nil {
				return err
			}
			if err := goldenio.WriteFile(out, e.Name(), data); err != nil {
				return err
			}
			copied++
		}
		if copied == 0 {
			return fmt.Errorf("no case file of another architecture in %s", *compare)
		}
	}
	return goldenio.WriteFile(out, name, []byte(own))
}
