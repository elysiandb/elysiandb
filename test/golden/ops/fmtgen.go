package main

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"math"
	"math/rand"
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func genFmt(out string) error {
	var l goldenio.Lines
	floats := []float64{0, math.Copysign(0, -1), 1, 0.5, 0.0005, 0.0004999, 1.0005, 2.675, 1e-7, 1e21, 1e20, 123456.789, 99.9995, -0.0005, 1.5e-4, 3.14159265, 1e300, math.MaxFloat64, math.NaN(), math.Inf(1), math.Inf(-1)}
	r := rand.New(rand.NewSource(17))
	for i := 0; i < 300; i++ {
		floats = append(floats, r.Float64()*math.Pow(10, float64(r.Intn(16)-6)))
	}
	for _, f := range floats {
		rounded := math.Round(f*1000) / 1000
		row := map[string]any{"fn": "metrics-float", "bits": fmt.Sprintf("%016x", math.Float64bits(f)), "prometheus_g": strconv.FormatFloat(f, 'g', -1, 64), "bucket_le": strconv.FormatFloat(f/1000, 'g', -1, 64)}
		if !math.IsNaN(f) && !math.IsInf(f, 0) {
			b, err := metrics.Number(rounded).MarshalJSON()
			if err != nil {
				return err
			}
			row["series_json"] = string(b)
		}
		l.Add(row)
	}
	durs := []time.Duration{0, 1, 999, time.Microsecond, 1500 * time.Microsecond, time.Millisecond, 250 * time.Millisecond, 1500 * time.Millisecond, time.Minute + 30*time.Second, 2 * time.Hour, -time.Second, 1<<63 - 1}
	for _, d := range durs {
		l.Add(map[string]any{"fn": "duration", "ns": strconv.FormatInt(int64(d), 10), "string": d.String(), "round_ms": d.Round(time.Millisecond).String(), "round_s": d.Round(time.Second).String(), "ms": d.Milliseconds()})
	}
	for _, s := range []string{"plain", "a b", "a=b", "say \"hi\"", "a\\b", "a\nb", "a\tb", "a\x01b", "a\u00a0b", "a\u200bb", "a\xffb", "\U0001f600", ""} {
		l.Add(map[string]any{"fn": "quote", "in": hs(s), "out": strconv.Quote(s)})
	}
	lits, err := bannerLiterals()
	if err != nil {
		return err
	}
	var b strings.Builder
	fmt.Fprintf(&b, lits[0], globals.Blue)
	fmt.Fprintln(&b, lits[1])
	fmt.Fprintf(&b, lits[2], globals.Gold+globals.Bold, lits[3], globals.Reset+globals.Blue)
	fmt.Fprintf(&b, lits[4], globals.Gray, lits[5], globals.Reset+globals.Blue)
	fmt.Fprintf(&b, lits[6], globals.Gold, lits[7], globals.Reset+globals.Blue)
	fmt.Fprintln(&b, lits[8]+globals.Reset)
	l.Add(map[string]any{"fn": "banner", "hex": hs(b.String())})
	return l.Write(out, "fmt/cases.jsonl")
}

func bannerLiterals() ([]string, error) {
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, "elysiandb.go", nil, 0)
	if err != nil {
		return nil, err
	}
	var lits []string
	for _, d := range f.Decls {
		fd, ok := d.(*ast.FuncDecl)
		if !ok || fd.Name.Name != "banner" {
			continue
		}
		ast.Inspect(fd.Body, func(n ast.Node) bool {
			if bl, ok := n.(*ast.BasicLit); ok && bl.Kind == token.STRING {
				if v, err := strconv.Unquote(bl.Value); err == nil {
					lits = append(lits, v)
				}
			}
			return true
		})
	}
	if len(lits) != 9 {
		return nil, fmt.Errorf("banner has %d string literals, want 9", len(lits))
	}
	return lits, nil
}

func hs(s string) string {
	return fmt.Sprintf("%x", s)
}
