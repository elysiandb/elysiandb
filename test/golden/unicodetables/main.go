package main

import (
	"bytes"
	"flag"
	"fmt"
	"os"
	"sort"
	"strings"
	"unicode"
)

const (
	pC = 1 << iota
	pP
	pN
	pS
	pZ
	pLu
	pLl
	pp
	pg  = pp | pZ
	pLo = pLl | pLu
)

const (
	indent     = "    "
	maxWidth   = 100
	listWidth  = maxWidth - len(indent) - 1
	tupleWidth = 60
)

type group struct {
	name   string
	prefix string
	tables map[string]*unicode.RangeTable
}

type foldPair struct {
	from, to rune
}

func main() {
	out := flag.String("out", "", "path of the generated Rust file")
	flag.Parse()
	if *out == "" || flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "usage: unicodetables -out <file>")
		os.Exit(2)
	}
	var b bytes.Buffer
	if err := generate(&b); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := os.WriteFile(*out, b.Bytes(), 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func generate(b *bytes.Buffer) error {
	groups := []group{
		{"CATEGORIES", "", unicode.Categories},
		{"SCRIPTS", "", unicode.Scripts},
		{"PROPERTIES", "", unicode.Properties},
		{"FOLD_CATEGORY", "FOLD_", unicode.FoldCategory},
		{"FOLD_SCRIPT", "FOLD_", unicode.FoldScript},
	}
	seen := map[string]bool{}
	for _, id := range []string{"VERSION", "CASE_RANGES", "LATIN1_PROPERTIES", "ASCII_FOLD", "CASE_ORBIT"} {
		seen[id] = true
	}
	for _, g := range groups {
		seen[g.name] = true
	}
	for _, g := range groups {
		for name := range g.tables {
			id := rustName(g.prefix, name)
			if seen[id] {
				return fmt.Errorf("duplicate Rust name %s", id)
			}
			seen[id] = true
		}
	}
	props, err := latin1Properties()
	if err != nil {
		return err
	}
	ascii, orbit, err := foldTables()
	if err != nil {
		return err
	}
	b.WriteString("use super::{CaseRange, FoldPair, Range16, Range32, RangeTable, UPPER_LOWER};\n")
	b.WriteString("\n")
	fmt.Fprintf(b, "pub const VERSION: &str = %q;\n", unicode.Version)
	for _, g := range groups {
		writeGroup(b, g)
	}
	writeCaseRanges(b)
	b.WriteString("\npub(super) static LATIN1_PROPERTIES: [u8; 256] = [\n")
	items := make([]string, len(props))
	for i, p := range props {
		items[i] = fmt.Sprintf("0x%02x", p)
	}
	writeMixed(b, items)
	b.WriteString("];\n")
	b.WriteString("\npub(super) static ASCII_FOLD: [u16; 128] = [\n")
	items = make([]string, len(ascii))
	for i, f := range ascii {
		items[i] = fmt.Sprintf("0x%04x", f)
	}
	writeMixed(b, items)
	b.WriteString("];\n")
	b.WriteString("\npub(super) static CASE_ORBIT: &[FoldPair] = &[\n")
	for _, f := range orbit {
		fmt.Fprintf(b, "%sFoldPair::new(0x%04x, 0x%04x),\n", indent, f.from, f.to)
	}
	b.WriteString("];\n")
	return nil
}

func rustName(prefix, name string) string {
	return prefix + strings.ToUpper(name)
}

func sortedNames(m map[string]*unicode.RangeTable) []string {
	names := make([]string, 0, len(m))
	for name := range m {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}

func writeGroup(b *bytes.Buffer, g group) {
	names := sortedNames(g.tables)
	fmt.Fprintf(b, "\npub static %s: &[(&str, &RangeTable)] = &[\n", g.name)
	for _, name := range names {
		key := fmt.Sprintf("%q", name)
		value := "&" + rustName(g.prefix, name)
		if len(key)+len(", ")+len(value) > tupleWidth {
			fmt.Fprintf(b, "%s(\n%s%s%s,\n%s%s%s,\n%s),\n", indent, indent, indent, key, indent, indent, value, indent)
			continue
		}
		fmt.Fprintf(b, "%s(%s, %s),\n", indent, key, value)
	}
	b.WriteString("];\n")
	for _, name := range names {
		writeTable(b, rustName(g.prefix, name), g.tables[name])
	}
}

func writeTable(b *bytes.Buffer, id string, t *unicode.RangeTable) {
	fmt.Fprintf(b, "\npub static %s: RangeTable = RangeTable {\n", id)
	r16 := make([]string, len(t.R16))
	for i, r := range t.R16 {
		r16[i] = fmt.Sprintf("Range16::new(0x%04x, 0x%04x, %d)", r.Lo, r.Hi, r.Stride)
	}
	r32 := make([]string, len(t.R32))
	for i, r := range t.R32 {
		r32[i] = fmt.Sprintf("Range32::new(0x%04x, 0x%04x, %d)", r.Lo, r.Hi, r.Stride)
	}
	writeField(b, "r16", r16)
	writeField(b, "r32", r32)
	fmt.Fprintf(b, "%slatin_offset: %d,\n", indent, t.LatinOffset)
	b.WriteString("};\n")
}

func writeField(b *bytes.Buffer, field string, items []string) {
	switch len(items) {
	case 0:
		fmt.Fprintf(b, "%s%s: &[],\n", indent, field)
	case 1:
		fmt.Fprintf(b, "%s%s: &[%s],\n", indent, field, items[0])
	default:
		fmt.Fprintf(b, "%s%s: &[\n", indent, field)
		for _, it := range items {
			fmt.Fprintf(b, "%s%s%s,\n", indent, indent, it)
		}
		fmt.Fprintf(b, "%s],\n", indent)
	}
}

func writeMixed(b *bytes.Buffer, items []string) {
	lineLen := 0
	b.WriteString(indent)
	for _, it := range items {
		w := len(it) + 1
		if lineLen > 0 && lineLen+1+w > listWidth {
			b.WriteString("\n")
			b.WriteString(indent)
			lineLen = 0
		}
		if lineLen > 0 {
			b.WriteByte(' ')
			lineLen++
		}
		b.WriteString(it)
		b.WriteByte(',')
		lineLen += w
	}
	b.WriteString("\n")
}

func delta(d rune) string {
	if d == unicode.UpperLower {
		return "UPPER_LOWER"
	}
	return fmt.Sprintf("%d", d)
}

func writeCaseRanges(b *bytes.Buffer) {
	b.WriteString("\npub static CASE_RANGES: &[CaseRange] = &[\n")
	for _, c := range unicode.CaseRanges {
		fmt.Fprintf(b, "%sCaseRange::new(0x%04x, 0x%04x, [%s, %s, %s]),\n", indent, c.Lo, c.Hi, delta(c.Delta[0]), delta(c.Delta[1]), delta(c.Delta[2]))
	}
	b.WriteString("];\n")
}

func latin1Properties() ([]uint8, error) {
	props := make([]uint8, unicode.MaxLatin1+1)
	for c := rune(0); c <= unicode.MaxLatin1; c++ {
		var p uint8
		if unicode.IsControl(c) {
			p |= pC
		}
		if unicode.IsPunct(c) {
			p |= pP
		}
		if unicode.IsNumber(c) {
			p |= pN
		}
		if unicode.IsSymbol(c) {
			p |= pS
		}
		if unicode.Is(unicode.Z, c) {
			p |= pZ
		}
		switch {
		case unicode.IsUpper(c):
			p |= pLu
		case unicode.IsLower(c):
			p |= pLl
		case unicode.IsLetter(c):
			p |= pLo
		}
		if unicode.IsPrint(c) {
			p |= pp
		}
		checks := []struct {
			name string
			got  bool
			want bool
		}{
			{"IsControl", p&pC != 0, unicode.IsControl(c)},
			{"IsPunct", p&pP != 0, unicode.IsPunct(c)},
			{"IsNumber", p&pN != 0, unicode.IsNumber(c)},
			{"IsSymbol", p&pS != 0, unicode.IsSymbol(c)},
			{"IsUpper", p&pLo == pLu, unicode.IsUpper(c)},
			{"IsLower", p&pLo == pLl, unicode.IsLower(c)},
			{"IsLetter", p&pLo != 0, unicode.IsLetter(c)},
			{"IsPrint", p&pp != 0, unicode.IsPrint(c)},
			{"IsGraphic", p&pg != 0, unicode.IsGraphic(c)},
		}
		for _, ch := range checks {
			if ch.got != ch.want {
				return nil, fmt.Errorf("latin-1 properties of %#04x disagree with %s", c, ch.name)
			}
		}
		props[c] = p
	}
	return props, nil
}

func defaultFold(r rune) rune {
	if l := unicode.To(unicode.LowerCase, r); l != r {
		return l
	}
	return unicode.To(unicode.UpperCase, r)
}

func foldTables() ([]rune, []foldPair, error) {
	ascii := make([]rune, unicode.MaxASCII+1)
	for c := rune(0); c <= unicode.MaxASCII; c++ {
		ascii[c] = unicode.SimpleFold(c)
	}
	inOrbit := map[rune]bool{}
	for c := rune(0); c <= unicode.MaxRune; c++ {
		if inOrbit[c] {
			continue
		}
		f := unicode.SimpleFold(c)
		if f == c && defaultFold(c) == c {
			continue
		}
		orbit := []rune{c}
		for r := f; r != c; r = unicode.SimpleFold(r) {
			orbit = append(orbit, r)
		}
		special := len(orbit) > 2
		for _, r := range orbit {
			if unicode.SimpleFold(r) != defaultFold(r) {
				special = true
			}
		}
		if special {
			for _, r := range orbit {
				inOrbit[r] = true
			}
		}
	}
	var orbit []foldPair
	for c := rune(0); c <= unicode.MaxRune; c++ {
		if inOrbit[c] {
			if c > 0xFFFF || unicode.SimpleFold(c) > 0xFFFF {
				return nil, nil, fmt.Errorf("fold orbit entry %#x does not fit in 16 bits", c)
			}
			orbit = append(orbit, foldPair{c, unicode.SimpleFold(c)})
		}
	}
	for c := rune(-2); c <= unicode.MaxRune+2; c++ {
		if got, want := emulateSimpleFold(c, ascii, orbit), unicode.SimpleFold(c); got != want {
			return nil, nil, fmt.Errorf("fold tables give %#x for %#x, Go gives %#x", got, c, want)
		}
	}
	return ascii, orbit, nil
}

func emulateSimpleFold(r rune, ascii []rune, orbit []foldPair) rune {
	if r < 0 || r > unicode.MaxRune {
		return r
	}
	if int(r) < len(ascii) {
		return ascii[r]
	}
	i := sort.Search(len(orbit), func(i int) bool { return orbit[i].from >= r })
	if i < len(orbit) && orbit[i].from == r {
		return orbit[i].to
	}
	return defaultFold(r)
}
