package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"hash/fnv"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	_ "unsafe"

	"github.com/dop251/goja/ftoa"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

//go:linkname bigFtoa github.com/dop251/goja/ftoa.ftoa
func bigFtoa(d float64, mode int, biasUp bool, ndigits int, buf []byte) ([]byte, int)

//go:linkname fastDtoa github.com/dop251/goja/ftoa/internal/fast.Dtoa
func fastDtoa(v float64, mode int, requestedDigits int, buffer []byte) ([]byte, int, bool)

const (
	seed          = 20261124
	maxPrecision  = 101
	modeShortest  = 0
	modePrecision = 1
)

var compare = flag.String("compare", "", "folder written by the run on the other architecture: its files are kept and only the lines of this architecture that differ are written")

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("goja-ftoa", run)
}

type entry struct {
	group string
	v     float64
	index int
}

func bits(f float64) string {
	return fmt.Sprintf("%016x", math.Float64bits(f))
}

func parse(s string) float64 {
	f, err := strconv.ParseFloat(s, 64)
	if err != nil {
		panic(err)
	}
	return f
}

func pow2(e int) float64 {
	if e >= -1022 {
		return math.Float64frombits(uint64(e+1023) << 52)
	}
	return math.Float64frombits(1 << uint(e+1074))
}

func specialValues() []float64 {
	strs := []string{
		"0", "1", "0.1", "0.2", "0.3", "0.30000000000000004", "0.5", "1.5", "2.5", "3.5", "4.35", "8.85", "885", "25",
		"1.005", "1.45", "1.255", "1.0000000000000002", "0.9999999999999999", "99.99", "999.9999999999999",
		"9.5", "0.05", "0.15", "0.25", "0.35", "123.456", "255.5", "1234.5678", "0.8466400793967279",
		"1e-5", "1e-6", "1e-7", "1.5e-7", "9.99999e-7", "5e-7", "9.5367431640625e-7", "1.2345678901234567e-7",
		"1e15", "1e16", "1e17", "1e20", "9.999999999999999e20", "1e21", "1.2345678901234568e20", "1e22", "1e23",
		"1e100", "1e-100", "123e-20", "100", "4294967272", "4294967295", "4294967296", "2147483647", "2147483648",
		"9007199254740991", "9007199254740992", "9007199254740994", "9223372036854774784", "9223372036854775808",
		"9223372036854777856", "18446744073709551616", "1e300", "1e-300", "3.5844466002796428e+298",
		"4.1855804968213567e298", "5.5626846462680035e-309", "1.7976931348623157e308", "2.2250738585072014e-308",
		"2.225073858507201e-308", "5e-324", "1e-323", "4.9406564584124654e-324",
		"3.141592653589793", "2.718281828459045", "1.4142135623730951", "0.6931471805599453",
		"12345678901234567890", "1.2345678901234568e+29", "0.000001", "0.0000001", "1.7976931348623155e308",
		"6.103515625e-05", "0.1e-6", "7.2057594037927933e16", "4.5e15", "0.5e-6", "3e-7", "2e-323", "1e-310",
	}
	var out []float64
	for _, s := range strs {
		f := parse(s)
		out = append(out, f, -f)
	}
	out = append(out,
		math.NaN(), math.Inf(1), math.Inf(-1),
		1.0/3, 2.0/3, -1.0/3,
		math.Nextafter(1e21, 0), math.Nextafter(1e21, math.Inf(1)),
		math.Nextafter(1e-6, 0), math.Nextafter(1e-6, 1), math.Nextafter(1e-7, 0), math.Nextafter(1e-7, 1),
		math.Float64frombits(0x000FFFFFFFFFFFFF), math.Float64frombits(0x0010000000000000),
		math.Float64frombits(0x7FEFFFFFFFFFFFFF), math.Float64frombits(0x0000000000000001),
		math.Float64frombits(0x8000000000000001), math.Float64frombits(0x43E0000000000000),
		math.Float64frombits(0xC3E0000000000000), math.Float64frombits(0x43DFFFFFFFFFFFFF),
		math.Float64frombits(0x43E0000000000001), math.Float64frombits(0x4340000000000000),
		math.Float64frombits(0x433FFFFFFFFFFFFF), math.Float64frombits(0x3FEFFFFFFFFFFFFF),
	)
	return out
}

func randomDecimal(rng *rand.Rand) float64 {
	digits := 1 + rng.Intn(17)
	var b strings.Builder
	b.WriteByte(byte('1' + rng.Intn(9)))
	for i := 1; i < digits; i++ {
		b.WriteByte(byte('0' + rng.Intn(10)))
	}
	fmt.Fprintf(&b, "e%d", rng.Intn(61)-30)
	f := parse(b.String())
	if rng.Intn(2) == 0 {
		f = -f
	}
	return f
}

func randomBits(rng *rand.Rand) float64 {
	for {
		f := math.Float64frombits(rng.Uint64())
		if !math.IsNaN(f) && !math.IsInf(f, 0) && f != 0 {
			return f
		}
	}
}

func corpus() []entry {
	var out []entry
	for _, v := range specialValues() {
		out = append(out, entry{"special", v, 0})
	}
	for e := -307; e <= 308; e++ {
		p := parse("1e" + strconv.Itoa(e))
		out = append(out, entry{"pow10", math.Nextafter(p, 0), 0}, entry{"pow10", p, 0}, entry{"pow10", math.Nextafter(p, math.Inf(1)), 0})
	}
	for e := -1074; e <= 1023; e++ {
		p := pow2(e)
		if e < -1022 {
			out = append(out, entry{"pow2", p, 0})
			continue
		}
		out = append(out, entry{"pow2", math.Nextafter(p, 0), 0}, entry{"pow2", p, 0}, entry{"pow2", math.Nextafter(p, math.Inf(1)), 0})
	}
	rng := rand.New(rand.NewSource(seed))
	for i := 0; i < 8000; i++ {
		out = append(out, entry{"random-bits", randomBits(rng), 0})
	}
	for i := 0; i < 8000; i++ {
		out = append(out, entry{"random-decimal", randomDecimal(rng), 0})
	}
	var tens []float64
	for e := -21; e <= 21; e++ {
		tens = append(tens, parse("1e"+strconv.Itoa(e)))
	}
	for i := 0; i < 4000; i++ {
		f := rng.Float64() * tens[rng.Intn(len(tens))]
		if rng.Intn(2) == 0 {
			f = -f
		}
		out = append(out, entry{"random-unit", f, 0})
	}
	for n := 0; n < 1000; {
		f := math.Abs(randomBits(rng))
		if _, _, ok := fastDtoa(f, modeShortest, 0, nil); !ok {
			out = append(out, entry{"fallback-shortest", f, 0})
			n++
		}
	}
	for n := 0; n < 1000; {
		f := math.Abs(randomDecimal(rng))
		if _, _, ok := fastDtoa(f, modePrecision, 1+rng.Intn(17), nil); !ok {
			out = append(out, entry{"fallback-precision", f, 0})
			n++
		}
	}
	seen := map[string]int{}
	for i := range out {
		out[i].index = seen[out[i].group]
		seen[out[i].group]++
	}
	return out
}

type strRecord struct {
	V      string            `json:"v"`
	Group  string            `json:"group"`
	Std    string            `json:"std"`
	StdExp string            `json:"stdexp"`
	Fixed  map[string]string `json:"fixed"`
	Exp    map[string]string `json:"exp"`
	Prec   map[string]string `json:"prec"`
	Sweep  string            `json:"sweep"`
}

type baseRecord struct {
	V     string            `json:"v"`
	Group string            `json:"group"`
	Base  map[string]string `json:"base"`
}

type pathRecord struct {
	V     string         `json:"v"`
	Group string         `json:"group"`
	Fast  any            `json:"fast"`
	Big   any            `json:"big"`
	FastP map[string]any `json:"fastp"`
	BigP  map[string]any `json:"bigp"`
	Modes map[string]any `json:"modes"`
}

func fToStr(v float64, mode ftoa.FToStrMode, prec int) string {
	return string(ftoa.FToStr(v, mode, prec, nil))
}

func precisions(rng *rand.Rand, full bool) []int {
	if full {
		out := make([]int, 0, maxPrecision+1)
		for p := 0; p <= maxPrecision; p++ {
			out = append(out, p)
		}
		return out
	}
	return []int{rng.Intn(maxPrecision + 1)}
}

func sweep(v float64) string {
	h := fnv.New64a()
	write := func(s string) {
		h.Write([]byte(s))
		h.Write([]byte{0})
	}
	write(fToStr(v, ftoa.ModeStandard, 0))
	write(fToStr(v, ftoa.ModeStandardExponential, 0))
	for _, mode := range []ftoa.FToStrMode{ftoa.ModeFixed, ftoa.ModeExponential, ftoa.ModePrecision} {
		for p := 0; p <= maxPrecision; p++ {
			write(fToStr(v, mode, p))
		}
	}
	return fmt.Sprintf("%016x", h.Sum64())
}

func strLine(e entry, rng *rand.Rand) strRecord {
	full := e.group == "special"
	r := strRecord{
		V:      bits(e.v),
		Group:  e.group,
		Std:    fToStr(e.v, ftoa.ModeStandard, 0),
		StdExp: fToStr(e.v, ftoa.ModeStandardExponential, 0),
		Fixed:  map[string]string{},
		Exp:    map[string]string{},
		Prec:   map[string]string{},
	}
	for _, p := range precisions(rng, full) {
		r.Fixed[strconv.Itoa(p)] = fToStr(e.v, ftoa.ModeFixed, p)
	}
	for _, p := range precisions(rng, full) {
		r.Exp[strconv.Itoa(p)] = fToStr(e.v, ftoa.ModeExponential, p)
	}
	for _, p := range precisions(rng, full) {
		r.Prec[strconv.Itoa(p)] = fToStr(e.v, ftoa.ModePrecision, p)
	}
	if e.group == "special" || e.group == "pow10" || strings.HasPrefix(e.group, "fallback-") || e.index < 1000 {
		r.Sweep = sweep(e.v)
	}
	return r
}

func baseLine(e entry, rng *rand.Rand) baseRecord {
	r := baseRecord{V: bits(e.v), Group: e.group, Base: map[string]string{}}
	var radixes []int
	if e.group == "special" {
		for radix := 2; radix <= 36; radix++ {
			radixes = append(radixes, radix)
		}
	} else {
		radixes = []int{2 + rng.Intn(35)}
	}
	for _, radix := range radixes {
		r.Base[strconv.Itoa(radix)] = ftoa.FToBaseStr(e.v, radix)
	}
	return r
}

func guard(f func() []any) (res any) {
	defer func() {
		if x := recover(); x != nil {
			res = map[string]any{"panic": fmt.Sprint(x)}
		}
	}()
	return f()
}

func fastCall(v float64, mode, n int) any {
	return guard(func() []any {
		digits, decPt, ok := fastDtoa(v, mode, n, nil)
		return []any{string(digits), decPt, ok}
	})
}

func bigCall(v float64, mode int, biasUp bool, n int) any {
	return guard(func() []any {
		digits, decPt := bigFtoa(v, mode, biasUp, n, nil)
		return []any{string(digits), decPt}
	})
}

func pathLine(e entry, rng *rand.Rand) pathRecord {
	v := math.Abs(e.v)
	r := pathRecord{
		V:     bits(v),
		Group: e.group,
		Fast:  fastCall(v, modeShortest, 0),
		Big:   bigCall(v, 0, false, 0),
		FastP: map[string]any{},
		BigP:  map[string]any{},
		Modes: map[string]any{},
	}
	for _, n := range precisions(rng, e.group == "special") {
		key := strconv.Itoa(n)
		r.FastP[key] = fastCall(v, modePrecision, n)
		r.BigP[key] = bigCall(v, 2, true, n)
	}
	if e.group == "special" {
		for mode := 0; mode <= 9; mode++ {
			for _, n := range []int{-3, -1, 0, 1, 2, 5, 10, 17, 25} {
				for _, biasUp := range []bool{false, true} {
					r.Modes[fmt.Sprintf("%d,%d,%t", mode, n, biasUp)] = bigCall(v, mode, biasUp, n)
				}
			}
		}
	} else {
		for i := 0; i < 2; i++ {
			mode := rng.Intn(10)
			n := rng.Intn(29) - 3
			biasUp := rng.Intn(2) == 1
			r.Modes[fmt.Sprintf("%d,%d,%t", mode, n, biasUp)] = bigCall(v, mode, biasUp, n)
		}
	}
	return r
}

func marshal(v any) (string, error) {
	data, err := json.Marshal(v)
	return string(data), err
}

func run(out string) error {
	entries := corpus()
	files := map[string][]string{}
	order := []string{"ftostr.jsonl", "ftobasestr.jsonl", "paths.jsonl"}
	rng := rand.New(rand.NewSource(seed + 1))
	for _, e := range entries {
		line, err := marshal(strLine(e, rng))
		if err != nil {
			return err
		}
		files["ftostr.jsonl"] = append(files["ftostr.jsonl"], line)
		if math.IsNaN(e.v) || math.IsInf(e.v, 0) {
			continue
		}
		line, err = marshal(baseLine(e, rng))
		if err != nil {
			return err
		}
		files["ftobasestr.jsonl"] = append(files["ftobasestr.jsonl"], line)
		if e.v == 0 {
			continue
		}
		line, err = marshal(pathLine(e, rng))
		if err != nil {
			return err
		}
		files["paths.jsonl"] = append(files["paths.jsonl"], line)
	}
	for _, name := range order {
		if *compare == "" {
			if err := goldenio.WriteFile(out, name, []byte(strings.Join(files[name], "\n")+"\n")); err != nil {
				return err
			}
			continue
		}
		if err := writeDifferences(out, name, files[name]); err != nil {
			return err
		}
	}
	return nil
}

type key struct {
	V     string `json:"v"`
	Group string `json:"group"`
}

type difference struct {
	Line   int             `json:"line"`
	GOARCH string          `json:"goarch"`
	Record json.RawMessage `json:"record"`
}

func writeDifferences(out, name string, own []string) error {
	data, err := os.ReadFile(filepath.Join(*compare, name))
	if err != nil {
		return err
	}
	base := strings.Split(strings.TrimSuffix(string(data), "\n"), "\n")
	if len(base) != len(own) {
		return fmt.Errorf("%s: %d lines in %s, %d computed", name, len(base), *compare, len(own))
	}
	var diff bytes.Buffer
	for i := range own {
		if own[i] == base[i] {
			continue
		}
		var a, b key
		if err := json.Unmarshal([]byte(own[i]), &a); err != nil {
			return err
		}
		if err := json.Unmarshal([]byte(base[i]), &b); err != nil {
			return err
		}
		if a != b {
			return fmt.Errorf("%s:%d: corpus differs between architectures", name, i+1)
		}
		line, err := marshal(difference{Line: i, GOARCH: runtime.GOARCH, Record: json.RawMessage(own[i])})
		if err != nil {
			return err
		}
		diff.WriteString(line)
		diff.WriteByte('\n')
	}
	if err := goldenio.WriteFile(out, name, data); err != nil {
		return err
	}
	if diff.Len() == 0 {
		return nil
	}
	return goldenio.WriteFile(out, "arch-"+runtime.GOARCH+"-"+name, diff.Bytes())
}
