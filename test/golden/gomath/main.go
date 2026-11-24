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
	mathCases(&b)
	return b.String()
}

var unary = []struct {
	name string
	f    func(float64) float64
}{
	{"sin", math.Sin},
	{"cos", math.Cos},
	{"tan", math.Tan},
	{"asin", math.Asin},
	{"acos", math.Acos},
	{"atan", math.Atan},
	{"sinh", math.Sinh},
	{"cosh", math.Cosh},
	{"tanh", math.Tanh},
	{"asinh", math.Asinh},
	{"acosh", math.Acosh},
	{"atanh", math.Atanh},
	{"cbrt", math.Cbrt},
	{"expm1", math.Expm1},
	{"log1p", math.Log1p},
	{"log2", math.Log2},
	{"log10", math.Log10},
	{"trunc", math.Trunc},
	{"ceil", math.Ceil},
	{"floor", math.Floor},
	{"sqrt", math.Sqrt},
}

var binary = []struct {
	name string
	f    func(float64, float64) float64
}{
	{"atan2", math.Atan2},
	{"max", math.Max},
	{"min", math.Min},
}

func mathCases(b *strings.Builder) {
	rng := rand.New(rand.NewSource(seed + 1))
	specials := []float64{0, 1, 0.5, 2, 3, math.Inf(1), math.NaN(), 1e-300, 5e-324, 2.2250738585072014e-308, 1e300, math.MaxFloat64,
		math.Pi, math.Pi / 2, math.Pi / 4, 3 * math.Pi / 4, 1e-9, 0.7, 0.66, 2.414213562373095, 0.625, 44.014845965556525, 44.02,
		21, 21.000001, 0.5000001, 0.3465735902799726, 1.0397207708399179, 38.816242111356935, 709.78, 709.79,
		1 << 28, 1<<28 + 1, 1.0 / (1 << 28), 1 << 29, 1<<29 - 1, 1 << 53, 0.41421356237309503, 0.2928932188134524,
		1.0 / (1 << 29), 1.0 / (1 << 54), 1e16, 1e22, 1e100, 1e200, 0.9999999999999999, 1.0000000000000002, 1e-20}
	var xs []float64
	for _, s := range specials {
		xs = append(xs, s, -s)
	}
	for i := 0; i < 200; i++ {
		xs = append(xs, (rng.Float64()-0.5)*100)
		xs = append(xs, rng.Float64()*2-1)
		xs = append(xs, (rng.Float64()-0.5)*8)
		m := math.Ldexp(0.5+rng.Float64()/2, rng.Intn(2100)-1075)
		if rng.Intn(2) == 0 {
			m = -m
		}
		xs = append(xs, m)
		xs = append(xs, (rng.Float64()-0.5)*1e10)
	}
	for _, u := range unary {
		for _, x := range xs {
			fmt.Fprintf(b, "%s %s %s\n", u.name, hex(x), hex(u.f(x)))
		}
	}
	pairs := [][2]float64{}
	edges := []float64{0, math.Copysign(0, -1), 1, -1, math.Inf(1), math.Inf(-1), math.NaN(), 2.5, -2.5}
	for _, y := range edges {
		for _, x := range edges {
			pairs = append(pairs, [2]float64{y, x})
		}
	}
	for i := 0; i < 1500; i++ {
		y := (rng.Float64() - 0.5) * 20
		x := (rng.Float64() - 0.5) * 20
		if i%7 == 0 {
			x = math.Ldexp(0.5+rng.Float64()/2, rng.Intn(200)-100)
		}
		pairs = append(pairs, [2]float64{y, x})
	}
	for _, f := range binary {
		for _, p := range pairs {
			fmt.Fprintf(b, "%s %s %s %s\n", f.name, hex(p[0]), hex(p[1]), hex(f.f(p[0], p[1])))
		}
	}
	targeted := []struct {
		name   string
		f      func(float64) float64
		lo, hi float64
	}{
		{"sin", math.Sin, -10, 10},
		{"cos", math.Cos, -10, 10},
		{"tan", math.Tan, -10, 10},
		{"asin", math.Asin, -1, 1},
		{"acos", math.Acos, -1, 1},
		{"atan", math.Atan, -5, 5},
		{"sinh", math.Sinh, -0.75, 0.75},
		{"tanh", math.Tanh, -0.75, 0.75},
		{"asinh", math.Asinh, -4, 4},
		{"acosh", math.Acosh, 1, 4},
		{"cbrt", math.Cbrt, -1000, 1000},
		{"expm1", math.Expm1, -40, 40},
		{"log1p", math.Log1p, -0.99, 3},
		{"log2", math.Log2, 0, 100},
	}
	for _, t := range targeted {
		for i := 0; i < 1500; i++ {
			x := t.lo + float64(rng.Float64()*(t.hi-t.lo))
			fmt.Fprintf(b, "%s %s %s\n", t.name, hex(x), hex(t.f(x)))
		}
	}
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
