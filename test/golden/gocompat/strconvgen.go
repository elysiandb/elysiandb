package main

import (
	"math"
	"math/rand"
	"strconv"
	"strings"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

type floatFormat struct {
	fmt  byte
	prec int
}

var floatFormats = []floatFormat{
	{'f', -1}, {'g', -1}, {'e', -1}, {'f', 0}, {'f', 1}, {'f', 2}, {'f', 3}, {'f', 6}, {'e', 3}, {'g', 3}, {'g', 17},
}

func genStrconv(out string) error {
	var ff goldenio.Lines
	corpus := floatCorpus()
	corpus = append(corpus, math.NaN())
	for _, f := range corpus {
		row := map[string]any{"bits": fbits(f)}
		for _, spec := range floatFormats {
			row[string(spec.fmt)+strconv.Itoa(spec.prec)] = strconv.FormatFloat(f, spec.fmt, spec.prec, 64)
		}
		row["g-1_32"] = strconv.FormatFloat(float64(float32(f)), 'g', -1, 32)
		ff.Add(row)
	}
	if err := ff.Write(out, "strconv/format_float.jsonl"); err != nil {
		return err
	}
	var pf goldenio.Lines
	seen := map[string]bool{}
	addParse := func(s string) {
		if seen[s] {
			return
		}
		seen[s] = true
		v, err := strconv.ParseFloat(s, 64)
		row := map[string]any{"in": hs(s), "bits": fbits(v), "err": errKind(err)}
		v32, err32 := strconv.ParseFloat(s, 32)
		row["bits32"] = fbits(v32)
		row["err32"] = errKind(err32)
		pf.Add(row)
	}
	for _, s := range parseFloatInputs() {
		addParse(s)
	}
	for i, f := range corpus {
		if i%3 != 0 {
			continue
		}
		g := strconv.FormatFloat(f, 'g', -1, 64)
		addParse(g)
		addParse(strings.ToUpper(g))
		addParse(strconv.FormatFloat(f, 'e', 25, 64))
		addParse(strconv.FormatFloat(f, 'f', -1, 64))
	}
	r := rand.New(rand.NewSource(3))
	for i := 0; i < 1500; i++ {
		var b strings.Builder
		if r.Intn(4) == 0 {
			b.WriteByte('-')
		}
		for n := 1 + r.Intn(30); n > 0; n-- {
			b.WriteByte(byte('0' + r.Intn(10)))
		}
		if r.Intn(2) == 0 {
			b.WriteByte('.')
			for n := r.Intn(30); n > 0; n-- {
				b.WriteByte(byte('0' + r.Intn(10)))
			}
		}
		if r.Intn(2) == 0 {
			b.WriteByte('e')
			b.WriteString(strconv.Itoa(r.Intn(700) - 350))
		}
		addParse(b.String())
	}
	if err := pf.Write(out, "strconv/parse_float.jsonl"); err != nil {
		return err
	}
	var pi goldenio.Lines
	for _, s := range parseIntInputs {
		v, err := strconv.ParseInt(s, 10, 64)
		u32, eu32 := strconv.ParseUint(s, 10, 32)
		u64, eu64 := strconv.ParseUint(s, 10, 64)
		a, ea := strconv.Atoi(s)
		i0, ei0 := strconv.ParseInt(s, 0, 64)
		pi.Add(map[string]any{
			"in":             hs(s),
			"int_10_64":      i64s(v),
			"int_10_64_err":  errKind(err),
			"uint_10_32":     u64s(u32),
			"uint_10_32_err": errKind(eu32),
			"uint_10_64":     u64s(u64),
			"uint_10_64_err": errKind(eu64),
			"atoi":           i64s(int64(a)),
			"atoi_err":       errKind(ea),
			"int_0_64":       i64s(i0),
			"int_0_64_err":   errKind(ei0),
		})
	}
	if err := pi.Write(out, "strconv/parse_int.jsonl"); err != nil {
		return err
	}
	var fi goldenio.Lines
	ints := []int64{0, 1, -1, 9, 10, 35, 36, 99, 100, 255, 256, 1 << 31, -1 << 31, 1<<53 + 1, math.MaxInt64, math.MinInt64, 1800000000, -1700000000}
	for _, v := range ints {
		fi.Add(map[string]any{
			"in": i64s(v), "b10": strconv.FormatInt(v, 10), "b36": strconv.FormatInt(v, 36), "b16": strconv.FormatInt(v, 16), "b2": strconv.FormatInt(v, 2),
			"u10": strconv.FormatUint(uint64(v), 10), "u36": strconv.FormatUint(uint64(v), 36), "itoa": strconv.Itoa(int(v)),
			"append": string(strconv.AppendInt([]byte("x"), v, 10)), "bool": strconv.FormatBool(v != 0),
		})
	}
	if err := fi.Write(out, "strconv/format_int.jsonl"); err != nil {
		return err
	}
	var q goldenio.Lines
	for _, s := range quoteInputs() {
		q.Add(map[string]any{"in": hs(s), "quote": strconv.Quote(s), "append": string(strconv.AppendQuote([]byte("x"), s)), "ascii": strconv.QuoteToASCII(s)})
	}
	return q.Write(out, "strconv/quote.jsonl")
}

func parseFloatInputs() []string {
	return []string{
		"", " 1", "1 ", "+1", "-1", "+-1", "1_000", "0x_1p0", "0x1_0p0", "1__0", "_1", "1_", "0x1p-2", "0X1.8P+1", "0x1.fffffffffffffp1023",
		"0x1p1024", "0x1p-1074", "0x1p-1075", "0x1.0000000000001p-1075", "0x1", "0x1.8", "0x", "0x.p1", "0xp1", "0x1p", "0x1e3",
		"inf", "+Inf", "-inf", "infinity", "-Infinity", "INFINITY", "infin", "nan", "NaN", "NAN", "-nan", "+nan",
		"1e400", "-1e400", "1e-400", "-1e-400", "1e308", "1.7976931348623157e308", "1.7976931348623158e308", "1.7976931348623159e308",
		"4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "2.2250738585072011e-308", "2.2250738585072012e-308",
		".5", "5.", ".", "e5", "1e", "1e+", "1e-", "1E5", "1e+05", "1e-05", "00001", "-0", "+0", "0.0", "-0.0", "0e0", "0e-400", "0e400",
		"9007199254740993", "9007199254740992.5", "18446744073709551616", "123456789012345678901234567890",
		"0.1", "0.10000000000000000555", "1,5", "1.5.5", "1e5e5", "0b101", "0o17", "1i", "- 1", "1e999999999999999999",
		"0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000049406564584124654",
		strings.Repeat("1", 800), strings.Repeat("9", 400) + "e-400", "0." + strings.Repeat("0", 330) + "1",
		"\xff", "1\x00", "\u0661",
	}
}

var parseIntInputs = []string{
	"", "0", "-0", "+0", "+5", "-5", "007", "08", "0x10", "0X1f", "0o17", "0b101", "0_1", "1_000", "_1", "1__0",
	"9223372036854775807", "9223372036854775808", "-9223372036854775808", "-9223372036854775809", "18446744073709551615", "18446744073709551616",
	"4294967295", "4294967296", "2147483647", "2147483648", " 1", "1 ", "abc", "-", "+", "1e3", "1.0", "\xff", "99999999999999999999999",
	"-18446744073709551615", "0x", "0xg", "00", "+-1",
}

func quoteInputs() []string {
	out := []string{"", "a", "\"", "\\", "'", "`", "\x00", "\x07\x08\x0c\x0a\x0d\x09\x0b", "a\xffb", "\xed\xa0\x80", "\xc0\xaf", "\xf4\x90\x80\x80"}
	for b := 0; b < 256; b++ {
		out = append(out, string([]byte{byte(b)}))
	}
	for c := rune(0x80); c <= 0x10ffff; c += 997 {
		if c >= 0xd800 && c <= 0xdfff {
			continue
		}
		out = append(out, string(c))
	}
	for _, c := range []rune{0xa0, 0xad, 0x2028, 0x2029, 0xfeff, 0xfffd, 0xe000, 0x10ffff, 0x1f600, 0x300, 0x200b, 0x200d, 0x061c, 0x180e, 0x3000, 0x1680} {
		out = append(out, string(c), "x"+string(c)+"y")
	}
	r := rand.New(rand.NewSource(5))
	for i := 0; i < 300; i++ {
		b := make([]byte, 1+r.Intn(12))
		for j := range b {
			b[j] = byte(r.Intn(256))
		}
		out = append(out, string(b))
	}
	return out
}
