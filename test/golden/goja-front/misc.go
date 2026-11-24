package main

import (
	"unicode"

	"github.com/dop251/goja/parser"
	"github.com/dop251/goja/token"
	"github.com/taymour/elysiandb/test/golden/goldenio"
	"golang.org/x/text/unicode/rangetable"
)

func tableJSON(t *unicode.RangeTable) map[string]any {
	r16 := [][3]uint16{}
	for _, r := range t.R16 {
		r16 = append(r16, [3]uint16{r.Lo, r.Hi, r.Stride})
	}
	r32 := [][3]uint32{}
	for _, r := range t.R32 {
		r32 = append(r32, [3]uint32{r.Lo, r.Hi, r.Stride})
	}
	return map[string]any{"r16": r16, "r32": r32, "latin_offset": t.LatinOffset}
}

func genRangetable(out string) error {
	idNeg := rangetable.Merge(unicode.Pattern_Syntax, unicode.Pattern_White_Space)
	idStart := rangetable.Merge(unicode.Letter, unicode.Nl, unicode.Other_ID_Start)
	idCont := rangetable.Merge(idStart, unicode.Mn, unicode.Mc, unicode.Nd, unicode.Pc, unicode.Other_ID_Continue)
	var lines goldenio.Lines
	lines.Add(map[string]any{"name": "id_neg", "table": tableJSON(idNeg)})
	lines.Add(map[string]any{"name": "id_start", "table": tableJSON(idStart)})
	lines.Add(map[string]any{"name": "id_cont", "table": tableJSON(idCont)})
	lines.Add(map[string]any{"name": "empty", "table": tableJSON(rangetable.Merge())})
	lines.Add(map[string]any{"name": "latin_greek", "table": tableJSON(rangetable.Merge(unicode.Latin, unicode.Greek, unicode.Nd))})
	lines.Add(map[string]any{"name": "upper_lower", "table": tableJSON(rangetable.Merge(unicode.Upper, unicode.Lower))})
	lines.Add(map[string]any{"name": "self", "table": tableJSON(rangetable.Merge(unicode.Lu, unicode.Lu))})
	if err := lines.Write(out, "rangetable/merge.jsonl"); err != nil {
		return err
	}
	var ids goldenio.Lines
	var start, part []int32
	for r := rune(0x80); r <= unicode.MaxRune; r++ {
		s := parser.IsIdentifier(string([]rune{r}))
		p := parser.IsIdentifier("a" + string([]rune{r}))
		if s {
			start = append(start, r)
		}
		if p {
			part = append(part, r)
		}
	}
	ids.Add(map[string]any{"kind": "start", "count": len(start), "ranges": runRanges(start)})
	ids.Add(map[string]any{"kind": "part", "count": len(part), "ranges": runRanges(part)})
	return ids.Write(out, "rangetable/identifiers.jsonl")
}

func runRanges(rs []int32) [][2]int32 {
	out := [][2]int32{}
	for i := 0; i < len(rs); {
		j := i
		for j+1 < len(rs) && rs[j+1] == rs[j]+1 {
			j++
		}
		out = append(out, [2]int32{rs[i], rs[j]})
		i = j + 1
	}
	return out
}

func genToken(out string) error {
	var lines goldenio.Lines
	for t := token.Token(0); t <= 110; t++ {
		lines.Add(map[string]any{"t": int(t), "s": t.String(), "is_id": token.IsId(t), "unreserved": token.IsUnreservedWord(t)})
	}
	words := []string{"if", "in", "of", "do", "var", "let", "for", "new", "try", "this", "else", "case", "void", "with", "async", "await", "yield", "const", "while", "break", "catch", "throw", "class", "super", "return", "typeof", "delete", "switch", "static", "default", "finally", "extends", "function", "continue", "debugger", "instanceof", "enum", "export", "import", "implements", "interface", "package", "private", "protected", "public", "true", "false", "null", "undefined", "get", "set", "target", "", "If", "x"}
	for _, w := range words {
		k, strict := token.IsKeyword(w)
		lines.Add(map[string]any{"word": w, "t": int(k), "strict": strict})
	}
	return lines.Write(out, "token/tokens.jsonl")
}

var regexpPatterns = []string{
	"", "abc", "\\abc", "\\a\\b\\c", "\\x", "\\c", "\\cA", "\\cz", "\\ca", "\\cj", "\\ck", "\\c1", "\\+", "[\\b]", "\\b", "\\B",
	"\\u0z01\\x\\undefined", "\\\\|'|\\r|\\n|\\t|\\u2028|\\u2029", "]", "}", "%", "(%)", "(?:[%\\s])", "[[]", "\\101", "\\51",
	"\\051", "\\175", "\\0", "\\04", "\\00", "\\000", "\\0000", "\\08", "\\377", "\\400", "(.)^", "\\$", "[G-b]", "[G-b\\0]", "\\k",
	"\\x20", "\\x2", "\\x2g", "😊", "^.*", "(\\n)", "(a(bc))", "[]", "[^]", "[]a", "[^]a", "\\s+", "\\S+", "[\\s]", "[\\S]",
	"[", "(", "\\(?=)", ")", "0:(?)", "(?)", "(?U)", "(?)|(?i)", "(?P<w>)(?P<w>)(?P<D>)", "<%([\\s\\S]+?)%>", "(?<=y)x",
	"(?<!y)x", "(?!test)", "(?=test)", "\\1", "\\8", "\\9", "(a)\\1", "\\12", "\\u{1d306}", "\\u{1d306", "\\u{}", "\\u{zz}",
	"\\u1234", "\\u12", "\\u{12345678}", ".", "a.b", "[.]", "(.)", "(?:.)", "\\d\\D\\w\\W", "\\f\\n\\r\\t\\v", "\\/", "a/b",
	"[a-z]+@[a-z]+\\.[a-z]{2,}", "^(([^<>()\\[\\]\\\\.,;:\\s@\"]+(\\.[^<>()\\[\\]\\\\.,;:\\s@\"]+)*)|(\".+\"))@((\\[[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}])|(([a-zA-Z\\-0-9]+\\.)+[a-zA-Z]{2,}))$",
	"\\p{L}", "\\P{L}", "\\q", "\\é", "é", "[é-ü]", "\\-", "[\\-]", "\\]", "\\}", "\\{", "a{2,3}", "a{,3}", "(?<name>x)",
	"\\k<name>", "(?:a|b)*", "x*?", "\\u{10FFFF}", "\\u{110000}", "[\\u{1F600}-\\u{1F64F}]", "\\cJ\\cM", "a\\", "\\", "[\\",
	"[a\\]]", "(?", "(?:", "(?=", "(a", "a)", "a]", "[a", "\\x{41}",
}

func regexpRecord(pattern string, dotAll, unicode bool) map[string]any {
	res, err := parser.TransformRegExp(pattern, dotAll, unicode)
	m := map[string]any{"pattern": pattern, "dot_all": dotAll, "unicode": unicode}
	switch e := err.(type) {
	case nil:
		m["result"] = res
	case parser.RegexpErrorIncompatible:
		m["incompatible"] = e.Error()
	case parser.RegexpSyntaxError:
		m["syntax"] = e.Error()
	default:
		m["other"] = true
	}
	return m
}

func genRegexp(out string) error {
	var lines goldenio.Lines
	seen := map[string]bool{}
	for _, p := range regexpPatterns {
		if seen[p] {
			continue
		}
		seen[p] = true
		for _, dotAll := range []bool{false, true} {
			for _, unicode := range []bool{false, true} {
				lines.Add(regexpRecord(p, dotAll, unicode))
			}
		}
	}
	return lines.Write(out, "regexp/transform.jsonl")
}
