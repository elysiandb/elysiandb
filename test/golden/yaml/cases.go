package main

import (
	"fmt"
	"os"
	"reflect"
	"strings"
	"unicode/utf16"

	"github.com/taymour/elysiandb/internal/configuration"
)

var scalarTargets = []string{"iface", "int", "bool", "string", "strings", "optbool", "struct"}
var documentTargets = []string{"iface", "struct", "struct-strict"}
var configTargets = []string{"config", "config-strict"}

func scalarForms() []string {
	forms := []string{
		"", "~", "null", "Null", "NULL", "nUll", "NuLL", "!!null ~", "!!null ''", "!!null null", "!!null x", "''", `""`, "!!str ~", "!!str", "&a ~", "!!null",
		"y", "Y", "yes", "Yes", "YES", "true", "True", "TRUE", "on", "On", "ON",
		"n", "N", "no", "No", "NO", "false", "False", "FALSE", "off", "Off", "OFF",
		"yEs", "tRUE", "oN", "nO", "'yes'", `"true"`, "!!bool yes", "!!bool true", "!!bool 1", "!!bool ~", "!!str true", "!!str yes", "! yes", "!<tag:yaml.org,2002:bool> on",
		"0", "-0", "+0", "7", "-7", "+7", "010", "0777", "08", "09", "089", "0o17", "0O17", "0o8", "0x1F", "0X1F", "0x1f", "-0x1F", "+0x1F", "0xg", "0x",
		"0b101", "0B101", "-0b101", "+0b101", "0b2", "0b", "-0b", "1_000", "1__000", "_1000", "1000_", "0x_1F", "0b_101", "-0b_101", "0_7",
		"9223372036854775807", "9223372036854775808", "-9223372036854775808", "-9223372036854775809", "18446744073709551615", "18446744073709551616",
		"99999999999999999999", "0x7FFFFFFFFFFFFFFF", "0x8000000000000000", "0xFFFFFFFFFFFFFFFF", "0x10000000000000000",
		"0b" + strings.Repeat("1", 63), "0b1" + strings.Repeat("0", 63), "0b" + strings.Repeat("1", 64), "0b1" + strings.Repeat("0", 64),
		"-0b1" + strings.Repeat("0", 63), "-0b1" + strings.Repeat("0", 62) + "1", "-0b" + strings.Repeat("1", 64),
		"0.", ".0", ".5", "-.5", "+.5", "1.", "1.5", "-1.5", "8089.7", "1e3", "1E3", "1e+3", "1e-3", "1.5e3", "1e400", "-1e400", "1e-400", "4.9e-324",
		"1_0.5", "1.5_0", "1_e3", "0x1p4", "0x1.8p1", "1,000", "12:30", "1:20:30", "190:20:30",
		".inf", ".Inf", ".INF", "+.inf", "+.Inf", "-.inf", "-.Inf", "-.INF", ".iNf", "inf", "Infinity", ".nan", ".NaN", ".NAN", "-.nan", "+.nan", "nan", "NaN",
		".5.5", "+", "-", ".", "..", "1.5.5", "-0.0", "0.0", "9.3e18", "-9.3e18", "1e19", "-1e19", "2e18", "1.9e18", "123456789012345678901234567890",
		"0.1", "3.14159", "6.02e23", "1e308", "1.8e308", "-1e20", "9.2e18", "9.223372036854775e18", "-9.223372036854775808e18", "-9.2233720368547758e18",
		"2026-01-01", "2026-1-2", "2026-01-01T10:00:00Z", "2026-01-01t10:00:00Z", "2026-01-01 10:00:00", "2026-01-01T10:00:00.5Z",
		"2026-01-01T10:00:00.123456789+02:00", "2026-01-01T10:00:00.1234567891Z", "2026-02-30", "2026-13-01", "2026-00-10", "2026-01-01T25:00:00Z",
		"2026-01-01T10:60:00Z", "2026-01-01T10:00:00", "2026-01-01 10:00:00Z", "20260-01-01", "2026-01", "2026-01-01x", "2001-12-14 21:59:43.10 -5",
		"1999-12-31T23:59:60Z", "2026-01-01T10:00:00+0200", "2026-01-01T10:00:00+02", "2026-1-2T3:4:5Z", "2026-01-01T10:00Z", "0000-01-01",
		"-2026-01-01", "+2026-01-01", "2026-01-01 1:2:3.4", "2024-02-29", "2023-02-29", "'2026-01-01'", "!!timestamp 2026-01-01", "!!timestamp abc",
		"!!timestamp 12", "!!str 2026-01-01", "2026-01-01T10:00:00ZZ", "2026-01-01  10:00:00", "2026-01-01T10:00:00.Z", "2026-01-01T10:00:00,5Z",
		"abc", "a b c", "'single'", "'it''s'", "'multi\n  line'", `"double"`, `"esc\t\n\x41\u00e9\U0001F600"`, `"\0\a\b\v\f\r\e\ \"\\\N\_\L\P"`,
		"\"tab\\\tx\"", "\"line\\\n  continued\"", "\"multi\n  line\"", "\"multi\n\n  para\"", "plain\n  multi line", "plain\n\n  para",
		"|\n  literal\n  text\n", "|-\n  strip\n", "|+\n  keep\n\n", ">\n  folded\n  text\n", ">\n  folded\n\n  para\n   more\n  end\n", ">-\n  x\n",
		"|2\n   indented\n", "|1-\n  x\n", "|-1\n  x\n", "| # comment\n  x\n", "|\n\n  leading\n", "|\n", ">\n", ">+\n\n", "|\n  a\n   b\n  c\n",
		`"\x80"`, `"\u0000"`, `"\U0010FFFF"`, `"\xff"`, "<<", `"<<"`, "!!merge <<", "!!merge x", "!foo bar", "! 12", "!<tag:yaml.org,2002:int> 12",
		"!<tag:yaml.org,2002:str> 12", "!<!> x", "!!binary aGVsbG8=", "!!binary gIGC", `!!binary "not base64!"`, "!!binary |\n  aGVs\n  bG8=\n", "!!binary ''",
		"!!str 12", `!!int "12"`, "!!int '0x1F'", "!!int 'abc'", "!!int 1.5", "!!int ''", "!!int ~", "!!int 0b101", "!!int 18446744073709551615",
		"!!float 3", `!!float "3"`, "!!float 18446744073709551615", "!!float abc", "!!float .inf", "!!float 1e400", "!!float 0x1F", "!!float 2026-01-01",
		"!!seq a", "!!map a", "!custom x", "%TAG !e! tag:example.com,2000:\n--- !e!foo x", "%YAML 1.1\n--- x", "--- x", "--- |\n  x\n", "x\n...\n",
		"x # comment", "x #", "x#y", "é", "日本", "\U0001F600", "\u00a0x", "x\u2028y", `"\u2028"`, "x\u0085y", "&a x", "&a [x]", "!!str &a x", "&a !!str x",
		"- a", "- 1\n- true", "[a, b]", "[1, 2]", "[a, {b: c}]", "[[a]]", "{a: b}", "[]", "{}", "a: b", "? a\n: b", "y: n", "1: 2",
		"a: 1\nb: x", "d: [a, b]", "e: maybe", "f: {x: 1}", "g: [{x: 1}, {x: z}]", "h: {k: v}",
	}
	return forms
}

func scalarCases() []ycase {
	var cs []ycase
	for _, f := range scalarForms() {
		cs = append(cs, ycase{name: "form " + f, input: []byte(f + "\n"), targets: scalarTargets})
	}
	for _, f := range []string{"", "~", "x", "7", "0x1F", "'q'", "\"q\""} {
		cs = append(cs, ycase{name: "no newline " + f, input: []byte(f), targets: scalarTargets})
	}
	return cs
}

func utf16le(s string, bom bool) []byte {
	var b []byte
	if bom {
		b = append(b, 0xff, 0xfe)
	}
	for _, u := range utf16.Encode([]rune(s)) {
		b = append(b, byte(u), byte(u>>8))
	}
	return b
}

func utf16be(s string, bom bool) []byte {
	var b []byte
	if bom {
		b = append(b, 0xfe, 0xff)
	}
	for _, u := range utf16.Encode([]rune(s)) {
		b = append(b, byte(u>>8), byte(u))
	}
	return b
}

func documentCases() []ycase {
	var cs []ycase
	add := func(name, input string) {
		cs = append(cs, ycase{name: name, input: []byte(input), targets: documentTargets})
	}
	addRaw := func(name string, input []byte) {
		cs = append(cs, ycase{name: name, input: input, targets: documentTargets})
	}
	add("all fields", "a: 7\nb: text\nc: yes\nd: [x, y]\ne: false\nf:\n  x: 3\n  y: [p]\ng:\n  - x: 1\n  - x: 2\n    y: [q, r]\nh: {k: [1, 2.5, ~]}\n")
	add("field errors", "a: abc\nb: [x]\nc: maybe\nd: x\ne: 2\nf: 3\ng: [1, {x: z}]\n")
	add("nested errors", "f:\n  x: [1]\n  y: {a: b}\ng:\n  - y: q\n  - x: 1.5\n")
	add("item errors", "d: [a, [b], {c: d}, 3, ~, e]\nf:\n  y: [1, [2]]\n")
	add("several documents", "a: 1\n---\na: 2\n")
	add("explicit documents", "--- \na: 1\n...\n--- \na: 2\n")
	add("second document broken", "a: 1\n--- [bad\n")
	add("second document tab", "a: 1\n---\n\t: x\n")
	add("document end only", "a: 1\n...\n")
	add("anchor alias", "a: &x 7\nh: *x\n")
	add("alias line", "h: &x [1, 2]\na: *x\n")
	add("alias to scalar error line", "anchor: &s abc\n\na: *s\n")
	add("merge before", "base: &b {a: 1, b: x}\n<<: *b\nb: y\n")
	add("merge after", "base: &b {a: 1, b: x}\nb: y\n<<: *b\n")
	add("merge sequence", "m1: &m1 {a: 1}\nm2: &m2 {a: 2, b: z}\n<<: [*m1, *m2]\n")
	add("merge sequence inline", "<<: [{a: 1}, {b: w}]\n")
	add("merge inline", "<<: {a: 1, zz: 2}\n")
	add("merge empty sequence", "<<: []\na: 1\n")
	add("merge scalar", "<<: 1\n")
	add("merge sequence scalar", "<<: [1]\n")
	add("merge alias scalar", "s: &s 1\n<<: *s\n")
	add("merge sequence alias scalar", "s: &s 1\nm: &m {a: 1}\n<<: [*m, *s]\n")
	add("merge tagged", "!!merge <<: {a: 1}\n")
	add("merge quoted", "\"<<\": {a: 1}\n")
	add("merge nested", "base: &b {x: 4, y: [m]}\nf:\n  <<: *b\n  x: 5\n")
	add("merge errors", "base: &b {a: bad, c: maybe}\n<<: *b\n")
	add("merge unknown", "base: &b {a: 1, q: 2}\nf:\n  <<: *b\n  w: 3\n")
	add("merge dup", "base: &b {a: 1}\n<<: *b\na: 2\na: 3\n")
	add("duplicate keys", "a: 1\na: 2\nb: x\nb: y\n")
	add("duplicate sections", "f:\n  x: 1\n  q: 1\nf:\n  y: [a]\n  r: 2\n")
	add("duplicate map keys", "h:\n  k: 1\n  k: 2\n  1: a\n  1.0: b\n  -0.0: c\n  0.0: d\n  .nan: e\n  .nan: f\n  ~: g\n  null: h\n")
	add("unknown keys", "zz: 1\nf:\n  zz: [1, 2]\n  x: 1\ng:\n  - zz: 3\nyy: {a: b}\n")
	add("unknown keys of unknown", "zz:\n  yy: 1\n  ww: [2]\n")
	add("flow mapping", "{a: 1, b: [x, y], f: {x: 2}}\n")
	add("flow sequence of mappings", "g: [{x: 1}, {x: 2}]\n")
	add("flow trailing comma", "{a: 1, d: [x,],}\n")
	add("flow single pair", "d: [a: 1, b]\n")
	add("flow explicit keys", "{? a : 1, ? b}\n")
	add("flow sequence explicit key", "h: [? a : b, ? c]\n")
	add("flow key without value", "{a, b: c}\n")
	add("flow empty key", "{: a}\n")
	add("flow empty key in sequence", "h: [: a]\n")
	add("flow multiline", "d: [a,\n  b,\n  c]\nf: {x: 1\n  }\n")
	add("flow nested", "h: [a, [b, c], {d: e}, [[f]]]\n")
	add("flow quoted keys", "{\"a\": 1, 'b': x}\n")
	add("flow adjacent value", "{\"a\":1}\n")
	add("block sequence", "- a\n- b\n")
	add("block sequence of mappings", "-\n  a: 1\n- b: x\n")
	add("nested block sequences", "- - a\n  - b\n- c\n")
	add("indentless sequence", "d:\n- x\n- y\n")
	add("explicit block key", "? a\n: 1\n? b\n: x\n")
	add("explicit block key literal", "? |\n  block key\n: v\n")
	add("explicit key without value", "? a\n? b\n")
	add("empty values", "a:\nb:\nd:\n")
	add("deep block", "f:\n  y:\n    - p\n    - q\n  x: 9\n")
	add("comments", "# head\na: 1 # trailing\n# middle\n\nb: x # more\n# tail\n")
	add("crlf", "a: 1\r\nb: x\r\n")
	add("cr only", "a: 1\rb: x\r")
	add("nel breaks", "a: 1\u0085b: x\u0085")
	add("ls breaks", "a: 1\u2028b: x\u2028")
	add("ps breaks", "a: 1\u2029b: x\u2029")
	add("bom", "\ufeffa: 1\n")
	add("bom inside", "a: 1\n\ufeffb: x\n")
	add("bom at chunk", strings.Repeat("#", 600)+"\n\ufeffb: x\n")
	addRaw("utf16le bom", utf16le("a: 1\nb: é😀\n", true))
	addRaw("utf16be bom", utf16be("a: 1\nb: é😀\n", true))
	add("key types", "1: a\ntrue: b\n~: c\n1.5: d\n2001-01-01: e\n0x10: f\n")
	add("sequence key", "? [a]\n: b\n")
	add("mapping key", "? {a: 1}\n: b\n")
	add("flow sequence key", "{[a]: b}\n")
	add("alias key", "k: &k a\n*k : 5\n")
	add("alias key mapping", "k: &k {x: 1}\n*k : 5\n")
	add("anchor redefined", "x1: &x 1\nx2: &x 2\na: *x\n")
	add("anchors in sequence", "h: [&p 1, *p, &q [2], *q]\n")
	add("recursive anchor", "h: &a [*a]\n")
	add("recursive anchor mapping", "h: &a {k: *a}\n")
	add("recursive anchor nested", "a: &a\n  b: [*a]\n")
	add("tagged collections", "h: [!!map {a: 1}, !!seq [b], !!str {c: 2}, !foo [d]]\n")
	add("null mapping", "f: !!null {x: 1}\ne: !!null {x: 1}\nd: !!null [a]\nh: !!null {x: 1}\n")
	add("binary key", "!!binary YQ==: 9\n")
	add("binary value", "b: !!binary aGVsbG8=\nh: !!binary gIGC\n")
	add("timestamps in map", "h: {t: 2026-01-01, u: '2026-01-01', v: !!timestamp 2026-01-01T10:00:00Z}\n")
	add("quoted keys", "\"a\": 1\n'b': x\n")
	add("empty key", ": 1\n")
	add("null key", "~: 1\nnull: 2\n")
	add("plain multiline key value", "b: one\n  two\n  three\n")
	add("special plain", "b: a:b\nh: [a:b, c#d, -x, ?y, :z]\n")
	add("anchor on key and value", "&k a: &v 1\n")
	add("tag and anchor order", "b: !!str &t x\nh: &u !!str y\n")
	add("directive yaml", "%YAML 1.1\n---\na: 1\n")
	add("directive tag", "%TAG !e! tag:example.com,2000:app/\n---\nh: !e!foo bar\n")
	add("directive tag primary", "%TAG ! tag:example.com,2000:\n---\nh: !foo bar\n")
	add("directive tag secondary", "%TAG !! tag:example.com,2000:\n---\nh: !!int 3\nb: !!str x\n")
	add("verbatim tags", "h: [!<tag:yaml.org,2002:str> 1, !<!bar> 2]\n")
	add("uri escapes in tags", "h: !e%21x 1\n")
	add("uri escapes utf8", "h: !%C3%A9 1\n")
	add("long key", strings.Repeat("k", 1000)+": 1\n")
	add("long plain", "b: "+strings.Repeat("v", 2000)+"\n")
	add("long quoted", "b: \""+strings.Repeat("w", 2000)+"\"\n")
	add("simple key across chunk", strings.Repeat("#", 505)+"\nab: 1\n")
	add("multibyte across chunk", strings.Repeat("x", 510)+": é\n")
	add("block scalar keep at end", "b: |+\n  x\n\n\n")
	add("block scalar indent indicator", "b: |2-\n    x\n   y\n")
	add("folded more indented", "b: >\n  a\n    b\n  c\n\n  d\n")
	add("literal trailing spaces", "b: |\n  a  \n  b\n")
	add("quoted folding", "b: \"a\n  b\n\n  c\\\n  d\"\n")
	add("single quoted folding", "b: 'a\n  b\n\n  c'\n")
	add("plain with tabs", "b: a\tb\n")
	add("tab after value", "a: 1\t\nb:\tx\n")
	add("tab separator in flow", "{a:\t1}\n")
	add("sequence entries with comments", "d:\n  - a # one\n  # between\n  - b\n")
	add("windows bom crlf", "\ufeffa: 1\r\nb: x\r\n")
	add("trailing document marker", "a: 1\n---\n")
	add("only document markers", "---\n...\n")
	add("question mark plain", "?a: 1\n")
	add("colon plain", ":a: 1\n")
	add("dash plain", "-a: 1\n")
	add("many keys", func() string {
		var b strings.Builder
		for i := 0; i < 300; i++ {
			fmt.Fprintf(&b, "k%d: %d\n", i, i)
		}
		return b.String()
	}())
	add("alias many", "x: &x [1, 2]\nh: [*x, *x, *x, *x, *x]\n")
	return cs
}

func syntaxCases() []ycase {
	var cs []ycase
	add := func(name, input string) {
		cs = append(cs, ycase{name: name, input: []byte(input)})
	}
	addRaw := func(name string, input []byte) {
		cs = append(cs, ycase{name: name, input: input})
	}
	add("unclosed flow sequence", "a: [1, 2\n")
	add("unclosed flow mapping", "a: {b: 1\n")
	add("unclosed flow line 1", "store: [this is: not: valid\n")
	add("unclosed flow later", "a: 1\nb: 2\nc: [x,\n  y\n")
	add("open bracket", "[\n")
	add("open brace", "{\n")
	add("mismatched flow", "a: [b, {c: d]\n")
	add("unclosed double quote", "a: \"abc\n")
	add("unclosed double quote later", "a: 1\nb: \"abc\n")
	add("unclosed single quote", "a: 'abc\n")
	add("unclosed quote eof", "\"abc")
	add("unclosed single eof", "'abc")
	add("bad indentation", "store:\n  folder: a\n folder2: b\n")
	add("bad sequence indentation", "a:\n  - b\n - c\n")
	add("mapping values not allowed", "a: b\n  c: d\n")
	add("mapping values not allowed line 1", "a: b: c\n")
	add("sequence then mapping", "- a\nb: c\n")
	add("mapping then sequence", "a: 1\n- b\n")
	add("tab indentation", "a:\n\tb: c\n")
	add("tab at start", "\ta: b\n")
	add("tab in block scalar", "a: |\n\tb\n")
	add("tab before sequence", "a:\n  - b\n\t- c\n")
	add("tab in plain continuation", "a: b\n\tc\n")
	add("invalid escape", "a: \"\\q\"\n")
	add("invalid escape line 2", "a: 1\nb: \"\\q\"\n")
	add("short hex escape", "a: \"\\x4\"\n")
	add("surrogate escape", "a: \"\\uD800\"\n")
	add("big escape", "a: \"\\U00110000\"\n")
	add("bad hex escape", "x: 1\na: \"\\xZZ\"\n")
	add("slash escape", "a: \"\\/\"\n")
	add("unknown anchor", "a: *x\n")
	add("unknown anchor root", "*x\n")
	add("unknown anchor in sequence", "a: [*x]\n")
	add("unknown anchor key", "? *x\n: 1\n")
	add("anchor used before definition", "a: *x\nb: &x 1\n")
	add("empty anchor", "&\n")
	add("empty alias", "a: *\n")
	add("bad anchor char", "&a{ x\n")
	add("bad alias char", "a: *x{\n")
	addRaw("invalid utf8 lead", []byte("a: \xff\n"))
	addRaw("invalid utf8 trailing", []byte("a: \xc3(\n"))
	addRaw("incomplete utf8", []byte("a: \xc3"))
	addRaw("overlong utf8", []byte("a: \xc0\x80\n"))
	addRaw("surrogate utf8", []byte("a: \xed\xa0\x80\n"))
	addRaw("too big utf8", []byte("a: \xf4\x90\x80\x80\n"))
	addRaw("control character", []byte("a: \x01\n"))
	addRaw("nul character", []byte("a: b\x00c\n"))
	addRaw("del character", []byte("a: \x7f\n"))
	addRaw("c1 control", []byte("a: \xc2\x80\n"))
	addRaw("noncharacter fffe", []byte("a: \xef\xbf\xbe\n"))
	addRaw("utf16 without bom", utf16le("store: {}\n", false))
	addRaw("utf16 odd length", append(utf16le("a: 1\n", true), 'x'))
	addRaw("utf16 lone low surrogate", append(utf16le("a: ", true), 0x00, 0xdc, '\n', 0))
	addRaw("utf16 high without low", append(utf16le("a: ", true), 0x00, 0xd8, 'x', 0, '\n', 0))
	addRaw("utf16 incomplete pair", append(utf16le("a: ", true), 0x00, 0xd8))
	addRaw("utf16be control", append(utf16be("a: ", true), 0, 1, 0, '\n'))
	addRaw("ops utf16", []byte("\xff\xfes\x00t\x00o\x00r\x00e\x00:\x00 \x00{\x00}\x00\n\x00"))
	addRaw("reader error before syntax", []byte("[\n"+strings.Repeat("x", 300)+"\xff\n"))
	addRaw("syntax before reader error", []byte("a: b: c\n"+strings.Repeat("x", 700)+"\xff\n"))
	addRaw("syntax before late reader error", []byte("[a\n"+strings.Repeat("#", 2000)+"\n\xff\n"))
	addRaw("reader error after chunk", []byte(strings.Repeat("#", 600)+"\n"+"a: \xff\n"))
	addRaw("reader error in first window", []byte("a: b: c\n"+strings.Repeat("x", 300)+"\xff\n"))
	addRaw("reader error in second window", []byte("a: b: c\n"+strings.Repeat("x", 600)+"\xff\n"))
	addRaw("utf16 reader error in first window", append(utf16le("a: b: c\n"+strings.Repeat("x", 200), true), 0x00, 0xdc))
	add("incompatible yaml", "%YAML 1.2\n--- a\n")
	add("duplicate yaml directive", "%YAML 1.1\n%YAML 1.1\n--- a\n")
	add("duplicate tag directive", "%TAG ! !a\n%TAG ! !b\n--- a\n")
	add("unknown directive", "%FOO bar\n--- a\n")
	add("short version", "%YAML 1\n--- a\n")
	add("long version", "%YAML 123.1\n--- a\n")
	add("missing version", "%YAML a.1\n--- a\n")
	add("tag directive missing prefix", "%TAG !e!\n--- a\n")
	add("missing directive name", "% x\n--- a\n")
	add("directive trailing text", "%YAML 1.1 x\n--- a\n")
	add("directive glued", "%YAML1.1\n--- a\n")
	add("directive without document", "%YAML 1.1\n")
	add("tag handle missing bang", "%TAG e! x\n--- a\n")
	add("verbatim tag unclosed", "!<x a\n")
	add("undefined tag handle", "!e!x a\n")
	add("bad uri escape", "!%zz x\n")
	add("bad leading uri octet", "!%ff x\n")
	add("bad trailing uri octet", "!%c3%41 x\n")
	add("tag glued", "!x{ y\n")
	add("indentation indicator zero", "a: |0\n x\n")
	add("indentation indicator zero after chomping", "a: |+0\n x\n")
	add("block scalar header text", "a: | x\n")
	add("document indicator in quotes", "\"a\n---\nb\"\n")
	add("document end in quotes", "'a\n...\nb'\n")
	add("missing dash", "a:\n  - b\n  c: d\n")
	add("closing brace", "{a: b}}\n")
	add("closing bracket", "]\n")
	add("closing brace alone", "}\n")
	add("text after quoted", "a: 'b'c\n")
	add("text after flow", "[a] b\n")
	add("at sign", "@a\n")
	add("backtick", "`a`\n")
	add("at value", "a: @b\n")
	add("flow after block entry", "- [a] - b\n")
	add("simple key too long", strings.Repeat("a", 1025)+": b\n")
	add("block entry not allowed", "a: - b\n")
	add("key not allowed", "a: ? b\n")
	add("flow mapping missing comma", "{a: 1 b: 2}\n")
	add("flow sequence missing comma", "[a b: c d]\n")
	add("flow mapping in sequence missing", "[a, {b: c} d]\n")
	add("block in flow", "store: {a: - b}\n")
	add("trailing garbage", "store: {}\n}\n")
	add("colon in key", "store:\n  fold:er: a\n")
	add("bad indentation deep", "a:\n  b:\n    c: 1\n   d: 2\n")
	add("anchor alias mix", "a: &x *y\n")
	add("tab in flow indentation", "a: [1,\n\t2]\n")
	add("tab plain after break", "a:\n  b\n\tc\n")
	return cs
}

type configKey struct {
	path []string
	kind string
}

func configKeys() (leaves []configKey, sections [][]string) {
	var walk func(t reflect.Type, prefix []string)
	walk = func(t reflect.Type, prefix []string) {
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			p := append(append([]string{}, prefix...), yamlKey(f))
			switch f.Type.Kind() {
			case reflect.Struct:
				sections = append(sections, p)
				walk(f.Type, p)
			case reflect.Int, reflect.Int64:
				leaves = append(leaves, configKey{p, "int"})
			case reflect.Bool:
				leaves = append(leaves, configKey{p, "bool"})
			case reflect.String:
				leaves = append(leaves, configKey{p, "str"})
			case reflect.Slice:
				leaves = append(leaves, configKey{p, "list"})
			case reflect.Ptr:
				leaves = append(leaves, configKey{p, "optbool"})
			default:
				panic("unexpected config field kind " + f.Type.String())
			}
		}
	}
	walk(reflect.TypeOf(configuration.Config{}), nil)
	return leaves, sections
}

func nest(path []string, value string) string {
	var b strings.Builder
	for i, k := range path {
		b.WriteString(strings.Repeat("  ", i))
		b.WriteString(k)
		b.WriteString(":")
		if i == len(path)-1 {
			if value != "" {
				b.WriteString(" ")
				b.WriteString(value)
			}
		}
		b.WriteString("\n")
	}
	return b.String()
}

var leafForms = []string{
	"yes", "off", "0x1F", "0o17", "017", "0b101", "-42", "1_000", "1.5", "-1.5", "1e3", ".inf", ".nan", "~", "", "2026-01-01",
	`"8"`, "'quoted'", "!!str 12", `!!int "12"`, "!!bool yes", "plain text", "[a, 1]", "{a: b}",
}

var sectionForms = []string{"3", "abc", "~", `""`, "[a]", "{}", "{zzz: 1}", "!!null {}", "[]"}

func configCases() []ycase {
	var cs []ycase
	add := func(name, input string) {
		cs = append(cs, ycase{name: name, input: []byte(input), targets: configTargets})
	}
	leaves, sections := configKeys()
	for _, l := range leaves {
		for _, f := range leafForms {
			add(fmt.Sprintf("leaf %s %s", strings.Join(l.path, "."), f), nest(l.path, f))
		}
	}
	for _, s := range sections {
		dotted := strings.Join(s, ".")
		for _, f := range sectionForms {
			add(fmt.Sprintf("section %s %s", dotted, f), nest(s, f))
		}
		add("section "+dotted+" block sequence", nest(s, "")+strings.Repeat("  ", len(s))+"- a\n")
		add("section "+dotted+" unknown child", nest(append(append([]string{}, s...), "zzz"), "1"))
		add("section "+dotted+" unknown subtree", nest(append(append([]string{}, s...), "zzz", "yyy"), "1"))
	}
	add("unknown root", "zzz: 1\n")
	add("empty", "")
	add("tilde", "~\n")
	add("document marker only", "---\n")
	add("comments only", "# nothing\n# more\n")
	add("several documents", "store:\n  shards: 2\n---\nstore:\n  shards: 3\n")
	add("top sequence", "- a\n- b\n")
	add("top scalar", "hello\n")
	add("top number", "42\n")
	add("server scalar", "server: 3\n")
	add("server sequence", "server:\n  - a\n")
	add("port mapping", "server:\n  http:\n    port:\n      a: 1\n")
	add("quoted true", "stats:\n  enabled: \"true\"\n")
	add("host number", "server:\n  http:\n    host: 127\n")
	add("token hex", "security:\n  authentication:\n    token: 0x10\n")
	add("mode yes", "security:\n  authentication:\n    mode: yes\n")
	add("mode on enabled", "security:\n  authentication:\n    enabled: true\n    mode: on\n")
	add("list mixed", "server:\n  http:\n    cors:\n      allowedOrigins: [1, true, ~]\n")
	add("list scalar", "server:\n  http:\n    cors:\n      allowedOrigins: x\n")
	add("list mapping item", "server:\n  http:\n    cors:\n      allowedOrigins: [a, {b: c}]\n")
	add("list block", "server:\n  http:\n    trustedProxies:\n      - 10.0.0.1\n      - ::1\n")
	add("list empty", "server:\n  http:\n    trustedProxies: []\n")
	add("list null", "server:\n  http:\n    trustedProxies: ~\n")
	add("null field", "log:\n  slowRequestMs: ~\n")
	add("null section", "log: ~\n")
	add("cancel null", "api:\n  query:\n    cancelOnDisconnect: ~\n")
	add("cancel maybe", "api:\n  query:\n    cancelOnDisconnect: maybe\n")
	add("cancel false", "api:\n  query:\n    cancelOnDisconnect: false\n")
	add("duplicate key", "store:\n  folder: /a\n  folder: /b\n")
	add("duplicate section", "store:\n  shards: 4\nstore:\n  folder: /x\n")
	add("duplicate section unknown inside", "store:\n  shards: 4\nstore:\n  zzz: 1\n  folder: /x\n")
	add("merge then override", "a: &a\n  port: 5\nserver:\n  http:\n    <<: *a\n    port: 6\n")
	add("override then merge", "a: &a\n  port: 5\nserver:\n  http:\n    port: 6\n    <<: *a\n")
	add("merge with unknown", "a: &a\n  port: 5\n  zzz: 1\nserver:\n  http:\n    <<: *a\n")
	add("merge whole section", "base: &base\n  http: {port: 1}\n  tcp: {port: 2}\nserver: *base\n")
	add("alias section", "h: &h {enabled: true, port: 9}\nserver:\n  http: *h\n  tcp: *h\n")
	add("several type errors", "store:\n  shards: abc\n  folder: [x]\nserver:\n  http:\n    port: 1.5e30\n    enabled: maybe\nlog: [a]\n")
	add("out of range float", "store:\n  memory:\n    limitMB: 99999999999999999999\n")
	add("out of range unsigned", "store:\n  memory:\n    limitMB: 18446744073709551615\n")
	add("float truncation", "server:\n  http:\n    port: 8089.7\n  tcp:\n    port: -1.5\n")
	add("int bases", "store:\n  shards: 0777\n  flushIntervalSeconds: 0o17\n  minFreeDiskMB: 0x1F\n  backup:\n    keep: 0b101\n    everyMinutes: +80\nserver:\n  http:\n    port: 8_089\n  tcp:\n    port: 9999999999\n")
	add("string into int", "server:\n  http:\n    port: \"8089\"\n  tcp:\n    port: !!str 12\n")
	add("tagged int", "server:\n  http:\n    port: !!int \"12\"\n")
	add("unknown keys everywhere", "server:\n  http:\n    cors:\n      allowed: [x]\n    bogus: 1\n  udp: {}\nstore:\n  json: 1\nauthentication: {}\n")
	add("unknown pprof", "monitoring:\n  pprof:\n    enabled: true\n")
	add("anchor holder", "a: &a {port: 5}\nserver:\n  http: *a\n")
	add("fatal tag", "store:\n  shards: !!int abc\n")
	add("fatal merge", "server:\n  http:\n    <<: 1\n")
	add("fatal recursive", "a: &a\n  b: *a\nserver: *a\n")
	add("fatal binary", "store:\n  folder: !!binary \"%%\"\n")
	add("sequence key", "store:\n  ? [a]\n  : 1\n")
	add("sequence key root", "? [a]\n: 1\n")
	add("null key", "store:\n  ~: 1\n")
	add("flow config", "{store: {folder: /f, shards: 3}, server: {http: {enabled: yes, port: 1}}}\n")
	add("windows line endings", "store:\r\n  folder: /w\r\n  shards: 2\r\n")
	add("bom config", "\ufeffstore:\n  folder: /bom\n")
	add("utf16 config", string(utf16le("store:\n  folder: /u16\n", true)))
	for _, c := range opsYAMLCases() {
		add("ops "+c.name, c.input)
	}
	for _, name := range []string{"elysian.yaml", "docker/elysian.yaml"} {
		b, err := os.ReadFile(name)
		if err != nil {
			panic(err)
		}
		add("shipped "+name, string(b))
	}
	return cs
}

type opsCase struct{ name, input string }

func opsYAMLCases() []opsCase {
	var cs []opsCase
	add := func(name, input string) { cs = append(cs, opsCase{name, input}) }
	add("minimal", "store:\n  folder: /data\n")
	for _, v := range []string{"y", "Y", "yes", "Yes", "YES", "true", "True", "TRUE", "on", "On", "ON", "n", "N", "no", "No", "NO", "false", "False", "FALSE", "off", "Off", "OFF", `"true"`, "1", "0", "~", "null", "!!bool yes", "!!str true"} {
		add("bool "+v, "stats:\n  enabled: "+v+"\n")
	}
	for _, v := range []string{"8", "010", "0o10", "0x10", "0b101", "-3", "+3", "1_000", "1.5", "1e3", "99999999999999999999", "18446744073709551615", "9223372036854775807", "-9223372036854775808", `"8"`, "abc", "~", "!!int 12", "!!float 3", "[]", "{}", ".inf", ".nan"} {
		add("int "+v, "store:\n  shards: "+v+"\n")
	}
	for _, v := range []string{"100", "-5", "1.5", "2e3", `"7"`, "0x1F", "~"} {
		add("int64 "+v, "store:\n  memory:\n    limitMB: "+v+"\n")
	}
	for _, v := range []string{"/data", `"/data dir"`, "'single'", "2026-01-01", "2026-01-01T10:00:00Z", "12", "true", "~", "|\n    multi\n    line", ">\n    folded\n    text", `"esc\taped\u00e9"`, "!!str 12", "[a, b]"} {
		add("string "+strings.ReplaceAll(v, "\n", " "), "store:\n  folder: "+v+"\n")
	}
	for _, v := range []string{"[a, b]", "\n    - a\n    - b", "[]", "~", "a", "[1, 2]", "[[a]]", "{a: b}"} {
		add("list "+strings.ReplaceAll(v, "\n", " "), "server:\n  http:\n    trustedProxies: "+v+"\n")
	}
	for _, v := range []string{"true", "false", "~", "maybe"} {
		add("pointer bool "+v, "api:\n  query:\n    cancelOnDisconnect: "+v+"\n")
	}
	add("anchors and merge", "base: &b\n  enabled: true\n  port: 9000\nserver:\n  http:\n    <<: *b\n    host: h\n")
	add("merge then override", "base: &b\n  port: 9000\nserver:\n  http:\n    port: 1\n    <<: *b\n")
	add("duplicate keys", "store:\n  shards: 4\n  shards: 8\n")
	add("duplicate sections", "store:\n  shards: 4\nstore:\n  folder: /x\n")
	add("unknown nested", "store:\n  json: 1\nsecurity:\n  authentication:\n    enabled: false\n  extra: x\n")
	add("unknown deep", "api:\n  cache:\n    enabled: true\n    size: 3\n")
	add("colon in key", "store:\n  fold:er: a\n")
	add("block in flow", "store: {a: - b}\n")
	add("trailing garbage", "store: {}\n}\n")
	add("document separators", "store:\n  shards: 2\n---\nstore:\n  shards: 3\n")
	add("log level bad", "log:\n  level: loud\n")
	add("durability spaced", "store:\n  durability: \" fsync \"\n")
	add("auth mode upper", "security:\n  authentication:\n    enabled: true\n    mode: \" USER \"\n")
	return cs
}

func deepCases() []ycase {
	var cs []ycase
	add := func(name, input string) {
		cs = append(cs, ycase{name: name, input: []byte(input), targets: []string{"iface", "config"}, shallow: true})
	}
	for _, n := range []int{10000, 10001} {
		add(fmt.Sprintf("flow sequences %d", n), strings.Repeat("[", n)+strings.Repeat("]", n)+"\n")
		add(fmt.Sprintf("flow mappings %d", n), strings.Repeat("{a: ", n)+"x"+strings.Repeat("}", n)+"\n")
		add(fmt.Sprintf("block sequences %d", n), strings.Repeat("- ", n)+"x\n")
		add(fmt.Sprintf("explicit keys %d", n), strings.Repeat("? ", n)+"x\n")
	}
	add("flow in block depth", "a:\n  b:\n    c: "+strings.Repeat("[", 9998)+strings.Repeat("]", 9998)+"\n")
	var lol strings.Builder
	lol.WriteString("a: &a [x, x, x, x, x, x, x, x, x]\n")
	prev := "a"
	for _, name := range []string{"b", "c", "d", "e", "f", "g"} {
		fmt.Fprintf(&lol, "%s: &%s [%s]\n", name, name, strings.TrimSuffix(strings.Repeat("*"+prev+", ", 9), ", "))
		prev = name
	}
	add("alias expansion", lol.String())
	var chain strings.Builder
	chain.WriteString("n0: &n0 [x]\n")
	for i := 1; i < 400; i++ {
		fmt.Fprintf(&chain, "n%d: &n%d [*n%d]\n", i, i, i-1)
	}
	add("alias chain", chain.String())
	var wide strings.Builder
	wide.WriteString("w: &w {k: v}\nl: [")
	for i := 0; i < 2000; i++ {
		if i > 0 {
			wide.WriteString(", ")
		}
		wide.WriteString("*w")
	}
	wide.WriteString("]\n")
	add("alias wide", wide.String())
	return cs
}
