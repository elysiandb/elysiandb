package main

import (
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"hash"
	"hash/fnv"
	"os"
	"strings"

	"github.com/taymour/elysiandb/test/golden/goldenio"
	"golang.org/x/text/cases"
	"golang.org/x/text/collate"
	"golang.org/x/text/language"
	"golang.org/x/text/transform"
	"golang.org/x/text/unicode/norm"
)

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("xtext", run)
}

func run(out string) error {
	for _, step := range []func(string) error{genNorm, genCollate, genCases} {
		if err := step(out); err != nil {
			return err
		}
	}
	return nil
}

type rng struct{ s uint64 }

func (r *rng) next() uint64 {
	r.s ^= r.s << 13
	r.s ^= r.s >> 7
	r.s ^= r.s << 17
	return r.s
}

func (r *rng) intn(n int) int {
	return int(r.next() % uint64(n))
}

func hs(s string) string {
	return hex.EncodeToString([]byte(s))
}

type digest struct{ h hash.Hash64 }

func newDigest() *digest {
	return &digest{fnv.New64a()}
}

func (d *digest) add(b []byte) {
	var n [4]byte
	binary.LittleEndian.PutUint32(n[:], uint32(len(b)))
	d.h.Write(n[:])
	d.h.Write(b)
}

func (d *digest) addString(s string) {
	d.add([]byte(s))
}

func (d *digest) addInt(v int) {
	d.add([]byte{byte(v + 1)})
}

func (d *digest) sum() string {
	return fmt.Sprintf("%016x", d.h.Sum64())
}

func build(r *rng, pool []string, maxItems int) string {
	n := 1 + r.intn(maxItems)
	var b strings.Builder
	for i := 0; i < n; i++ {
		p := pool[r.intn(len(pool))]
		if r.intn(40) == 0 {
			p = strings.Repeat(p, 25+r.intn(16))
		}
		b.WriteString(p)
	}
	return b.String()
}

func runes(lo, hi rune, f func(rune)) {
	for c := lo; c < hi; c++ {
		if c >= 0xd800 && c <= 0xdfff {
			continue
		}
		f(c)
	}
}

var forms = []norm.Form{norm.NFC, norm.NFD, norm.NFKC, norm.NFKD}

var normPool = []string{
	"a", "A", "e", "o", "u", "z", " ", "1", ".", "i",
	"\u00e9", "\u00c5", "\u01d6", "\u1e69", "\u01fa", "\u0251", "\u00e0", "\u1e9b",
	"\u0300", "\u0301", "\u0302", "\u0308", "\u0323", "\u0327", "\u031b", "\u0334", "\u0345",
	"\u05b0", "\u0591", "\u0e38", "\u0e48", "\u0f71", "\u0f72", "\u0f74", "\u0f80", "\u1dce",
	"\u302a", "\u3099", "\u094d", "\u20d2", "\u0340", "\u0344", "\u0f73", "\u0f75", "\u0f77", "\u0f81",
	"\u0b47", "\u0b3e", "\u0b57", "\u0dd9", "\u0dcf", "\u0ddf", "\u09c7", "\u09be", "\u0cc6",
	"\u0cd5", "\u0cc2", "\u1025", "\u102e", "\u0d46", "\u0d3e", "\u0d4a", "\u0fb2",
	"\u1100", "\u1112", "\u1113", "\u1161", "\u1175", "\u1176", "\u11a7", "\u11a8", "\u11c2", "\u11c3",
	"\uac00", "\ub098", "\uac01", "\ud7a3", "\ud7a4",
	"\ufb01", "\ufdfa", "\u2126", "\u212b", "\uff9e", "\uff76", "\u3300", "\u00bd", "\u2474", "\ufb2c",
	"\U0001d15e", "\U0001d160", "\u2adc", "\u0958", "\U0001109a", "\U00011099", "\U000110ba",
	"\U00010041", "\U00020041", "\U0001d1bb", "\u0391", "\u03b9", "\u0399", "\u03a9", "\u1fb3",
	"\u034f", "\u200d", "A\u030a", "\u03c9\u0345",
	"\xff", "\x80", "\xc3", "\xe1\x84", "\xed\xa0\x80", "\xc0\x80", "\xf4\x90\x80\x80", "\xcc",
}

var normMarks = []string{
	"\u0300", "\u0301", "\u0302", "\u0308", "\u030a", "\u0323", "\u0327", "\u0345",
	"\u3099", "\u1161", "\u11a8", "\U000110ba",
}

var normMarksHigh = []string{"\u0301", "\u0308", "\u3099"}

func spanCode(err error) int {
	switch err {
	case nil:
		return 0
	case transform.ErrEndOfSpan:
		return 1
	case transform.ErrShortSrc:
		return 2
	}
	return 3
}

func iterate(f norm.Form, s string) (segs []int, cat []byte, ok bool) {
	defer func() {
		if recover() != nil {
			segs, cat, ok = nil, nil, false
		}
	}()
	var it norm.Iter
	it.InitString(f, s)
	segs = []int{}
	for !it.Done() {
		seg := it.Next()
		segs = append(segs, len(seg))
		cat = append(cat, seg...)
	}
	return segs, cat, true
}

func genNorm(out string) error {
	r := &rng{0x9e3779b97f4a7c15}
	var lines goldenio.Lines
	for i := 0; i < 1200; i++ {
		maxItems := 10
		if i%40 == 0 {
			maxItems = 90
		}
		s := build(r, normPool, maxItems)
		split := r.intn(len(s) + 1)
		var outs []string
		appendOut := map[string]string{}
		var normal, normalBytes []bool
		var quick, first, last, nextEOF, next []int
		var span [][]int
		var iters [][]int
		iterOut := map[string]string{}
		for k, f := range forms {
			o := f.String(s)
			outs = append(outs, hs(o))
			normal = append(normal, f.IsNormalString(s))
			normalBytes = append(normalBytes, f.IsNormal([]byte(s)))
			quick = append(quick, f.QuickSpanString(s))
			first = append(first, f.FirstBoundaryInString(s))
			last = append(last, f.LastBoundary([]byte(s)))
			nextEOF = append(nextEOF, f.NextBoundaryInString(s, true))
			next = append(next, f.NextBoundaryInString(s, false))
			n1, e1 := f.SpanString(s, true)
			n2, e2 := f.Span([]byte(s), false)
			span = append(span, []int{n1, spanCode(e1), n2, spanCode(e2)})
			if a := string(f.AppendString(f.Bytes([]byte(s[:split])), s[split:])); a != o {
				appendOut[fmt.Sprint(k)] = hs(a)
			}
			segs, cat, ok := iterate(f, s)
			if !ok {
				iters = append(iters, nil)
				continue
			}
			iters = append(iters, segs)
			if string(cat) != o {
				iterOut[fmt.Sprint(k)] = hex.EncodeToString(cat)
			}
		}
		row := map[string]any{
			"in": hs(s), "out": outs, "normal": normal, "normal_bytes": normalBytes,
			"quick_span": quick, "first_boundary": first, "last_boundary": last,
			"next_boundary_eof": nextEOF, "next_boundary": next, "span": span,
			"split": split, "iter": iters,
		}
		if len(appendOut) > 0 {
			row["append_out"] = appendOut
		}
		if len(iterOut) > 0 {
			row["iter_out"] = iterOut
		}
		lines.Add(row)
	}
	if err := lines.Write(out, "norm/strings.jsonl"); err != nil {
		return err
	}
	var marks goldenio.Lines
	for block := rune(0); block < 0x110; block++ {
		list := normMarks
		if block >= 0x20 {
			list = normMarksHigh
		}
		for m, mark := range list {
			ds := []*digest{newDigest(), newDigest(), newDigest(), newDigest()}
			runes(block<<12, (block+1)<<12, func(c rune) {
				s := string(c) + mark
				for k, f := range forms {
					ds[k].addString(f.String(s))
				}
			})
			var sums []string
			for _, d := range ds {
				sums = append(sums, d.sum())
			}
			marks.Add(map[string]any{"block": block, "mark": m, "digests": sums})
		}
	}
	if err := marks.Write(out, "norm/marks.jsonl"); err != nil {
		return err
	}
	var invalid goldenio.Lines
	for b0 := 0; b0 < 256; b0++ {
		ds := []*digest{newDigest(), newDigest(), newDigest(), newDigest()}
		for b1 := 0; b1 < 256; b1++ {
			pair := string([]byte{byte(b0), byte(b1)})
			for _, s := range []string{pair, "a\u0301" + pair + "\u0300", pair + "\x80\x80", "\u1100" + pair + "\u11a8"} {
				for k, f := range forms {
					ds[k].addString(f.String(s))
				}
			}
		}
		var sums []string
		for _, d := range ds {
			sums = append(sums, d.sum())
		}
		invalid.Add(map[string]any{"b0": b0, "digests": sums})
	}
	return invalid.Write(out, "norm/invalid.jsonl")
}

var collatePool = []string{
	"a", "A", "b", "c", "l", "L", "z", "e", "\u00e9", "\u00c5", "\u00df", "\u00e6", "\u0153",
	"\u0438", "\u0418", "\u0435", "\u0415", "\u0443", "\u0456", "\u0433", "\u0306", "\u0308", "\u0301",
	"\u0627", "\u0648", "\u064a", "\u0653", "\u0654", "\u0655",
	"\u0fb2", "\u0fb3", "\u0f71", "\u0f72", "\u0f80", "\u0f81", "\u0f40",
	"\u0e40", "\u0e01", "\u0ec0", "\u0e81", "\u1025", "\u102e", "\u1b05", "\u1b35",
	"\uac00", "\ud7a3", "\u1100", "\u1161", "\u11a8",
	"\u4e00", "\u9fff", "\u3400", "\uf900", "\U00020000", "\ue000", "\U0010fffd", "\U00011131", "\U00011127",
	"\x00", "\u00ad", "\u200b", "\x01", " ", "-", "'", ".", "_",
	"1", "2", "10", "\u0663", "\uff11",
	"\ufb01", "\u2460", "\u3300", "\u212b", "\u1e9b", "\u01c4", "\u00bd", "\uff21",
	"\u0300", "\u0323", "\u0327", "\u0334", "\u05b0", "\u0e38", "\u0e48", "\u302e", "\u302f", "\u035e", "\u035f",
	"\xff", "\x80", "\xc3", "\xe1\x84", "\xed\xa0\x80", "\xc0\x80",
}

var collateMarks = []string{
	"\u0300", "\u0301", "\u0306", "\u0308", "\u0323", "\u0327", "\u0334", "\u0653", "\u0654", "\u0655",
	"\u05b0", "\u0e38", "\u0e48", "\u0f71", "\u0f72", "\u0f80", "\u102e", "\u1b35", "\u3099",
	"\U00011127", "\U000110ba", "\U0001133e", "\U000114b0", "\U000115af", "\U00011930",
	"\u0323\u0306", "\u0306\u0323", "\u0301\u0306", "\u0334\u0308", "\u0f71\u0f72", "\u0f72\u0f71", "\u0f80\u0f71",
}

func genCollate(out string) error {
	c := collate.New(language.Und)
	var buf collate.Buffer
	key := func(s string) []byte {
		buf.Reset()
		return c.KeyFromString(&buf, s)
	}
	r := &rng{0xd1b54a32d192ed03}
	var lines goldenio.Lines
	prev := ""
	for i := 0; i < 2000; i++ {
		maxItems := 6
		if i%50 == 0 {
			maxItems = 60
		}
		s := build(r, collatePool, maxItems)
		nfd := norm.NFD.String(s)
		lines.Add(map[string]any{
			"in":          hs(s),
			"key":         hex.EncodeToString(key(s)),
			"key_nfd":     hex.EncodeToString(key(nfd)),
			"compare":     c.CompareString(prev, s),
			"compare_nfd": c.CompareString(norm.NFD.String(prev), nfd),
		})
		prev = s
	}
	if err := lines.Write(out, "collate/strings.jsonl"); err != nil {
		return err
	}
	var single goldenio.Lines
	for block := rune(0); block < 0x110; block++ {
		dk, dn, dc, dcn := newDigest(), newDigest(), newDigest(), newDigest()
		runes(block<<12, (block+1)<<12, func(ch rune) {
			s, t := string(ch), string(ch+1)
			dk.add(key(s))
			dn.add(key(norm.NFD.String(s)))
			dc.addInt(c.CompareString(s, t))
			dcn.addInt(c.CompareString(norm.NFD.String(s), norm.NFD.String(t)))
		})
		single.Add(map[string]any{"block": block, "key": dk.sum(), "key_nfd": dn.sum(), "compare": dc.sum(), "compare_nfd": dcn.sum()})
	}
	if err := single.Write(out, "collate/runes.jsonl"); err != nil {
		return err
	}
	var marks goldenio.Lines
	for _, block := range []rune{0, 1, 2, 3} {
		for m, mark := range collateMarks {
			dk, dn, dc, dcn := newDigest(), newDigest(), newDigest(), newDigest()
			lo, hi := block<<12, (block+1)<<12
			if block == 3 {
				lo, hi = 0x10000, 0x20000
			}
			runes(lo, hi, func(ch rune) {
				s, t := string(ch)+mark, string(ch+1)+mark
				dk.add(key(s))
				dn.add(key(norm.NFD.String(s)))
				dc.addInt(c.CompareString(s, t))
				dcn.addInt(c.CompareString(norm.NFD.String(s), string(ch)))
			})
			marks.Add(map[string]any{"block": block, "mark": m, "key": dk.sum(), "key_nfd": dn.sum(), "compare": dc.sum(), "compare_nfd": dcn.sum()})
		}
	}
	if err := marks.Write(out, "collate/marks.jsonl"); err != nil {
		return err
	}
	var invalid goldenio.Lines
	for b0 := 0; b0 < 256; b0++ {
		dk, dc := newDigest(), newDigest()
		for b1 := 0; b1 < 256; b1++ {
			pair := string([]byte{byte(b0), byte(b1)})
			for _, s := range []string{pair, pair + "\x80", "a" + pair + "\u0301", "\u0438" + pair + "\u0306"} {
				dk.add(key(s))
				dc.addInt(c.CompareString(s, pair+"a"))
			}
		}
		invalid.Add(map[string]any{"b0": b0, "key": dk.sum(), "compare": dc.sum()})
	}
	return invalid.Write(out, "collate/invalid.jsonl")
}

var casesPool = []string{
	"a", "A", "z", "Z", " ", "1", ".", "'", "-", ":", "\u2019", "\u00ad", "\u02bc", "\u0301", "\u0345", "\u0307",
	"\u03a3", "\u03c3", "\u03c2", "\u0391", "\u03b1", "\u0399", "\u0390", "\u03b0", "\u1f80", "\u1fb3", "\u1fbc",
	"\u00df", "\u0130", "\u0131", "\u0149", "\u01f0", "\ufb03", "\ufb05", "\u01c5", "\u01c4", "\u01c6",
	"\u1e9e", "\u2c65", "\u023a", "\u0587", "\u10d0", "\u13a0", "\uab70", "\U00010400", "\U0001e922",
	"A\u0307", "i\u0307", "\u0130\u0301", "I", "J", "\u012e", "\u00cc",
	"\u3000", "\u4e00", "\u0660", "\u2160", "\u24b6", "\u24d0", "\uff21", "\uff41",
	"\xff", "\x80", "\xc3", "\xe1\x84", "\xed\xa0\x80", "\xc0\x80", "\xce",
}

var sigmaContext = []string{
	"", "a", "A", "\u03b1", "\u0391", " ", "'", "\u0301", "\u00ad", "1", "\u03a3", "a'", "a\u0301",
	"a\u00ad", "a ", ".", "\u2019", "a\u02bc", "\u0345", "\u01c5", "\ufb00", "\xff", "\u0301\u0301", "::",
}

func genCases(out string) error {
	upper := cases.Upper(language.Und)
	lower := cases.Lower(language.Und)
	lowerNoSigma := cases.Lower(language.Und, cases.HandleFinalSigma(false))
	row := func(s string) map[string]any {
		return map[string]any{
			"in":            hs(s),
			"upper":         hs(upper.String(s)),
			"lower":         hs(lower.String(s)),
			"lower_nosigma": hs(lowerNoSigma.String(s)),
		}
	}
	r := &rng{0x2545f4914f6cdd1d}
	var lines goldenio.Lines
	for i := 0; i < 2000; i++ {
		maxItems := 8
		if i%20 == 0 {
			maxItems = 200
		}
		lines.Add(row(build(r, casesPool, maxItems)))
	}
	if err := lines.Write(out, "cases/strings.jsonl"); err != nil {
		return err
	}
	var sigma goldenio.Lines
	for _, pre := range sigmaContext {
		for _, suf := range sigmaContext {
			for _, mid := range []string{"\u03a3", "\u03a3\u03a3", "\u03a3\u0301", "\u0301\u03a3"} {
				sigma.Add(row(pre + mid + suf))
			}
		}
	}
	for n := 118; n <= 136; n++ {
		pad := strings.Repeat("a", n)
		for _, suf := range []string{"", " ", "a", "\u0301\u0301", "''b", "'''", "\u03a3", "\u00ad\u00ad\u00ad a"} {
			sigma.Add(row(pad + "\u03a3" + suf))
			sigma.Add(row(pad[:n/2] + "\u03b1" + pad[n/2+2:] + "\u03a3" + suf))
		}
	}
	for k := 25; k <= 36; k++ {
		for _, ign := range []string{"\u0301", "'", "\u00ad"} {
			run := strings.Repeat(ign, k)
			sigma.Add(row("a\u03a3" + run + "a"))
			sigma.Add(row("a\u03a3" + run))
			sigma.Add(row("a\u03a3" + run + " "))
			sigma.Add(row(strings.Repeat("b", 100) + "\u03a3" + run + "c"))
		}
	}
	if err := sigma.Write(out, "cases/sigma.jsonl"); err != nil {
		return err
	}
	var context goldenio.Lines
	for block := rune(0); block < 0x110; block++ {
		dl, du := newDigest(), newDigest()
		runes(block<<12, (block+1)<<12, func(c rune) {
			s := string(c)
			for _, t := range []string{"A\u03a3" + s, "A\u03a3'" + s, s + "\u03a3", "A" + s + "\u03a3 ", "\u03a3" + s + "\u03a3"} {
				dl.addString(lower.String(t))
				du.addString(upper.String(t))
			}
		})
		context.Add(map[string]any{"block": block, "lower": dl.sum(), "upper": du.sum()})
	}
	if err := context.Write(out, "cases/context.jsonl"); err != nil {
		return err
	}
	var invalid goldenio.Lines
	for b0 := 0; b0 < 256; b0++ {
		dl, du, dn := newDigest(), newDigest(), newDigest()
		for b1 := 0; b1 < 256; b1++ {
			pair := string([]byte{byte(b0), byte(b1)})
			for _, s := range []string{pair, pair + "\x80\x80", "a\u03a3" + pair, "\u03a3" + pair + "a", "\u0130" + pair} {
				dl.addString(lower.String(s))
				du.addString(upper.String(s))
				dn.addString(lowerNoSigma.String(s))
			}
		}
		invalid.Add(map[string]any{"b0": b0, "lower": dl.sum(), "upper": du.sum(), "lower_nosigma": dn.sum()})
	}
	return invalid.Write(out, "cases/invalid.jsonl")
}
