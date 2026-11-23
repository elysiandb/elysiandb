package main

import (
	"encoding/hex"
	"fmt"
	"math"
	"math/rand"
	"os"
	"sort"
	"strconv"
	"unicode/utf8"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("gocompat", run)
}

func run(out string) error {
	steps := []func(string) error{
		genUnicode,
		genUTF,
		genStrings,
		genStrconv,
		genJSON,
		genFmt,
		genTime,
		genSort,
		genRand,
		genBase64,
	}
	for _, step := range steps {
		if err := step(out); err != nil {
			return err
		}
	}
	return nil
}

func hx(b []byte) string {
	return hex.EncodeToString(b)
}

func hs(s string) string {
	return hex.EncodeToString([]byte(s))
}

func fbits(f float64) string {
	return fmt.Sprintf("%016x", math.Float64bits(f))
}

func textOrHex(m map[string]any, field string, b []byte) {
	if utf8.Valid(b) {
		m[field] = string(b)
	} else {
		m[field+"_hex"] = hx(b)
	}
}

func i64s(v int64) string {
	return strconv.FormatInt(v, 10)
}

func u64s(v uint64) string {
	return strconv.FormatUint(v, 10)
}

func errKind(err error) string {
	if err == nil {
		return ""
	}
	if ne, ok := err.(*strconv.NumError); ok {
		switch ne.Err {
		case strconv.ErrSyntax:
			return "syntax"
		case strconv.ErrRange:
			return "range"
		}
		return "other"
	}
	return "error"
}

func floatCorpus() []float64 {
	seen := map[uint64]bool{}
	var out []float64
	add := func(f float64) {
		b := math.Float64bits(f)
		if seen[b] {
			return
		}
		seen[b] = true
		out = append(out, f)
	}
	specials := []float64{
		0, math.Copysign(0, -1), math.Inf(1), math.Inf(-1),
		math.Float64frombits(1), math.Float64frombits(0x000fffffffffffff), math.Float64frombits(0x0010000000000000),
		math.MaxFloat64, -math.MaxFloat64, math.SmallestNonzeroFloat64,
		1, -1, 0.1, 0.2, 0.3, 1.0 / 3, 2.0 / 3, -0.5, 0.5, 1.5, 2.5, 100, 123456789, 1e15, 1e16, 1e17,
		1e-7, 1e-6, 9.999999999999999e-7, 1.0000000000000002e-6, 1e20, 1e21, 9.999999999999999e20, 1.0000000000000001e21, 1e22, 1e23,
		1 << 53, 1<<53 + 2, 1 << 63, 1 << 64, 9007199254740991, -9007199254740991, 4294967295, 4294967296,
		math.Pi, math.E, math.Sqrt2, 299792458, 6.02214076e23, 1.602176634e-19, 5e-324, 1.7976931348623157e308,
	}
	for _, f := range specials {
		add(f)
		add(math.Nextafter(f, math.Inf(1)))
		add(math.Nextafter(f, math.Inf(-1)))
	}
	for e := -325; e <= 310; e++ {
		f, _ := strconv.ParseFloat("1e"+strconv.Itoa(e), 64)
		add(f)
		add(-f)
		g, _ := strconv.ParseFloat("5e"+strconv.Itoa(e), 64)
		add(g)
	}
	for i := int64(-20); i <= 20; i++ {
		add(float64(1<<53 + i))
	}
	r := rand.New(rand.NewSource(1))
	for i := 0; i < 3000; i++ {
		f := math.Float64frombits(r.Uint64())
		if math.IsNaN(f) {
			continue
		}
		add(f)
	}
	for i := 0; i < 2000; i++ {
		m := r.Int63n(10000000)
		e := r.Intn(41) - 20
		f, _ := strconv.ParseFloat(strconv.FormatInt(m, 10)+"e"+strconv.Itoa(e), 64)
		if r.Intn(2) == 0 {
			f = -f
		}
		add(f)
	}
	return out
}

func sortedKeys[V any](m map[string]V) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
