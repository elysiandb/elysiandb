package main

import (
	"fmt"
	"strconv"
	"strings"
	"unicode"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

type rng struct{ lo, hi rune }

func flatten(t *unicode.RangeTable) []rng {
	var pts []rng
	push := func(lo, hi rune) {
		if n := len(pts); n > 0 && pts[n-1].hi+1 == lo {
			pts[n-1].hi = hi
			return
		}
		pts = append(pts, rng{lo, hi})
	}
	for _, r := range t.R16 {
		if r.Stride == 1 {
			push(rune(r.Lo), rune(r.Hi))
			continue
		}
		for c := rune(r.Lo); c <= rune(r.Hi); c += rune(r.Stride) {
			push(c, c)
		}
	}
	for _, r := range t.R32 {
		if r.Stride == 1 {
			push(rune(r.Lo), rune(r.Hi))
			continue
		}
		for c := rune(r.Lo); c <= rune(r.Hi); c += rune(r.Stride) {
			push(c, c)
		}
	}
	return pts
}

func predicateRanges(f func(rune) bool) []rng {
	var pts []rng
	in := false
	var start rune
	for c := rune(0); c <= unicode.MaxRune+1; c++ {
		v := c <= unicode.MaxRune && f(c)
		if v && !in {
			start, in = c, true
		}
		if !v && in {
			pts = append(pts, rng{start, c - 1})
			in = false
		}
	}
	return pts
}

func writeRangeGroup(b *strings.Builder, name string, pts []rng) {
	for _, p := range pts {
		fmt.Fprintf(b, "%s\t%04x\t%04x\n", name, p.lo, p.hi)
	}
}

func writeTables(out, rel string, tables map[string]*unicode.RangeTable) error {
	var b strings.Builder
	for _, name := range sortedKeys(tables) {
		writeRangeGroup(&b, name, flatten(tables[name]))
	}
	return goldenio.WriteFile(out, rel, []byte(b.String()))
}

func genUnicode(out string) error {
	if err := goldenio.WriteJSON(out, "unicode/version.json", map[string]string{"unicode": unicode.Version}); err != nil {
		return err
	}
	var cs strings.Builder
	for c := rune(0); c <= unicode.MaxRune; c++ {
		lo, up, ti, fo := unicode.ToLower(c), unicode.ToUpper(c), unicode.ToTitle(c), unicode.SimpleFold(c)
		if lo == c && up == c && ti == c && fo == c {
			continue
		}
		fmt.Fprintf(&cs, "%04x\t%04x\t%04x\t%04x\t%04x\n", c, lo, up, ti, fo)
	}
	if err := goldenio.WriteFile(out, "unicode/case.tsv", []byte(cs.String())); err != nil {
		return err
	}
	preds := map[string]func(rune) bool{
		"IsSpace":           unicode.IsSpace,
		"IsControl":         unicode.IsControl,
		"IsLetter":          unicode.IsLetter,
		"IsDigit":           unicode.IsDigit,
		"IsNumber":          unicode.IsNumber,
		"IsUpper":           unicode.IsUpper,
		"IsLower":           unicode.IsLower,
		"IsTitle":           unicode.IsTitle,
		"IsPunct":           unicode.IsPunct,
		"IsSymbol":          unicode.IsSymbol,
		"IsMark":            unicode.IsMark,
		"IsGraphic":         unicode.IsGraphic,
		"IsPrint":           unicode.IsPrint,
		"strconv.IsPrint":   strconv.IsPrint,
		"strconv.IsGraphic": strconv.IsGraphic,
	}
	var pb strings.Builder
	for _, name := range sortedKeys(preds) {
		writeRangeGroup(&pb, name, predicateRanges(preds[name]))
	}
	if err := goldenio.WriteFile(out, "unicode/predicates.tsv", []byte(pb.String())); err != nil {
		return err
	}
	groups := []struct {
		rel    string
		tables map[string]*unicode.RangeTable
	}{
		{"unicode/categories.tsv", unicode.Categories},
		{"unicode/scripts.tsv", unicode.Scripts},
		{"unicode/properties.tsv", unicode.Properties},
		{"unicode/fold_category.tsv", unicode.FoldCategory},
		{"unicode/fold_script.tsv", unicode.FoldScript},
	}
	for _, g := range groups {
		if err := writeTables(out, g.rel, g.tables); err != nil {
			return err
		}
	}
	var cr strings.Builder
	for _, c := range unicode.CaseRanges {
		fmt.Fprintf(&cr, "%04x\t%04x\t%d\t%d\t%d\n", c.Lo, c.Hi, c.Delta[0], c.Delta[1], c.Delta[2])
	}
	return goldenio.WriteFile(out, "unicode/case_ranges.tsv", []byte(cr.String()))
}
