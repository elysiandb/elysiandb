package main

import (
	"strings"
	"unicode/utf16"
	"unicode/utf8"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

var specialRunes = []rune{
	0, 0x7f, 0x80, 0x7ff, 0x800, 0xffff, 0x10000, 0x10ffff, 0x110000, -1,
	0xd7ff, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0xe000, 0xfffd, 0xfeff, 0x2028, 0x2029,
}

func genUTF(out string) error {
	var dec goldenio.Lines
	conts := []byte{0x00, 0x7f, 0x80, 0x8f, 0x90, 0x9f, 0xa0, 0xbf, 0xc0, 0xff}
	addDec := func(b []byte) {
		r, n := utf8.DecodeRune(b)
		lr, ln := utf8.DecodeLastRune(b)
		dec.Add(map[string]any{
			"in": hx(b), "rune": r, "size": n, "last_rune": lr, "last_size": ln,
			"valid": utf8.Valid(b), "full": utf8.FullRune(b), "count": utf8.RuneCount(b),
		})
	}
	addDec(nil)
	for lead := 0; lead < 256; lead++ {
		addDec([]byte{byte(lead)})
		if lead < 0x80 {
			continue
		}
		for _, c1 := range conts {
			addDec([]byte{byte(lead), c1})
			for _, c2 := range []byte{0x80, 0xbf, 0x41} {
				addDec([]byte{byte(lead), c1, c2})
				addDec([]byte{byte(lead), c1, c2, 0x80})
			}
		}
	}
	for _, s := range []string{"a\xffb", "\xed\xa0\x80\xed\xb0\x80", "\xf4\x90\x80\x80", "\xc0\xaf", "\xe0\x80\xaf", "\xf0\x80\x80\xaf", "\u00e9", "\u20ac", "\U0001f600", "\xf0\x9f\x98"} {
		addDec([]byte(s))
	}
	if err := dec.Write(out, "utf8/decode.jsonl"); err != nil {
		return err
	}
	var enc goldenio.Lines
	for _, r := range specialRunes {
		enc.Add(map[string]any{"rune": r, "append": hx(utf8.AppendRune(nil, r)), "len": utf8.RuneLen(r), "valid": utf8.ValidRune(r)})
	}
	if err := enc.Write(out, "utf8/encode.jsonl"); err != nil {
		return err
	}
	var u16 goldenio.Lines
	for _, r := range specialRunes {
		r1, r2 := utf16.EncodeRune(r)
		u16.Add(map[string]any{"fn": "EncodeRune", "rune": r, "r1": r1, "r2": r2, "surrogate": utf16.IsSurrogate(r)})
	}
	pairs := [][2]rune{{0xd800, 0xdc00}, {0xdbff, 0xdfff}, {0xd800, 0x41}, {0x41, 0xdc00}, {0xdc00, 0xd800}, {0xd83d, 0xde00}}
	for _, p := range pairs {
		u16.Add(map[string]any{"fn": "DecodeRune", "r1": p[0], "r2": p[1], "rune": utf16.DecodeRune(p[0], p[1])})
	}
	seqs := [][]uint16{{0x41, 0xd83d, 0xde00}, {0xd83d}, {0xde00, 0x41}, {0xd83d, 0xd83d, 0xde00}, {}}
	for _, s := range seqs {
		u16.Add(map[string]any{"fn": "Decode", "in": s, "runes": utf16.Decode(s)})
	}
	for _, rs := range [][]rune{{0x41, 0x1f600}, {0xd800}, {0x110000, -1}, {}} {
		u16.Add(map[string]any{"fn": "Encode", "runes": rs, "out": utf16.Encode(rs)})
	}
	return u16.Write(out, "utf16/cases.jsonl")
}

var stringInputs = []string{
	"", "abc", "ABC", "Hello World", "  padded\t\n", "\u00a0nbsp\u00a0", "\u0085next\u0085", "\u1680ogham", "\u2000\u2001\u2002\u200a",
	"\u2028\u2029", "\u202f\u205f\u3000", "\ufeffbom", "\u180emvs", "\u200bzw", "\u0130stanbul", "ISTANBUL", "\u0131i", "\u0391\u03a3", "\u039f\u0394\u03a5\u03a3\u03a3\u0395\u03a5\u03a3", "\u1e9e", "\u00df", "\u01c5\u01c4\u01c6",
	"\u212a", "\ufb03", "\u216b\u24d0\u24b6", "\u01c4ungla", "\xffA", "a\xc0\xafb", "\xed\xa0\x80", "MiXeD123_-", "\u00c0\u00c9\u00ce\u00d5\u00dc", "\u00e0\u00e9\u00ee\u00f5\u00fc", "\u10a0", "\u2d00", "\uab70", "\u13a0",
	"\U00010400\U00010428", "\U0001e900\U0001e922", "\x00\x01\x7f", "\t\v\f\r ", "\u03a3\u03c3\u03c2", "\u01c8\u01c9\u01c7", "\u03a9\u2126", "\u0130\u0131", "\u00c5\u212b", "\u017f", "\ufb00", "\U0001f600",
}

func genStrings(out string) error {
	var l goldenio.Lines
	for _, s := range stringInputs {
		l.Add(map[string]any{
			"fn":         "Transform",
			"in":         hs(s),
			"to_lower":   hs(strings.ToLower(s)),
			"to_upper":   hs(strings.ToUpper(s)),
			"trim_space": hs(strings.TrimSpace(s)),
			"fields":     hexList(strings.Fields(s)),
			"valid":      hs(strings.ToValidUTF8(s, "\ufffd")),
		})
	}
	for i, a := range stringInputs {
		for j, b := range stringInputs {
			if strings.EqualFold(a, b) || (i+j)%7 == 0 {
				l.Add(map[string]any{"fn": "EqualFold", "a": hs(a), "b": hs(b), "equal": strings.EqualFold(a, b)})
			}
		}
	}
	return l.Write(out, "strings/cases.jsonl")
}

func hexList(ss []string) []string {
	out := make([]string, len(ss))
	for i, s := range ss {
		out[i] = hs(s)
	}
	return out
}
