package main

import (
	"encoding/hex"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
	"unicode/utf16"
	"unicode/utf8"

	"github.com/dlclark/regexp2"
	"github.com/dlclark/regexp2/syntax"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

const module = "github.com/dlclark/regexp2@v1.11.4"

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("regexp2", run)
}

func run(out string) error {
	steps := []func(string) error{
		genGoja,
		genCorpus,
		genReplace,
		genEscape,
		genStrings,
	}
	for _, step := range steps {
		if err := step(out); err != nil {
			return err
		}
	}
	return nil
}

var errNames = map[syntax.ErrorCode]string{
	syntax.ErrInternalError:              "InternalError",
	syntax.ErrUnterminatedComment:        "UnterminatedComment",
	syntax.ErrInvalidCharRange:           "InvalidCharRange",
	syntax.ErrInvalidRepeatSize:          "InvalidRepeatSize",
	syntax.ErrInvalidUTF8:                "InvalidUTF8",
	syntax.ErrCaptureGroupOutOfRange:     "CaptureGroupOutOfRange",
	syntax.ErrUnexpectedParen:            "UnexpectedParen",
	syntax.ErrMissingParen:               "MissingParen",
	syntax.ErrMissingBrace:               "MissingBrace",
	syntax.ErrInvalidRepeatOp:            "InvalidRepeatOp",
	syntax.ErrMissingRepeatArgument:      "MissingRepeatArgument",
	syntax.ErrConditionalExpression:      "ConditionalExpression",
	syntax.ErrTooManyAlternates:          "TooManyAlternates",
	syntax.ErrUnrecognizedGrouping:       "UnrecognizedGrouping",
	syntax.ErrInvalidGroupName:           "InvalidGroupName",
	syntax.ErrCapNumNotZero:              "CapNumNotZero",
	syntax.ErrUndefinedBackRef:           "UndefinedBackRef",
	syntax.ErrUndefinedNameRef:           "UndefinedNameRef",
	syntax.ErrAlternationCantCapture:     "AlternationCantCapture",
	syntax.ErrAlternationCantHaveComment: "AlternationCantHaveComment",
	syntax.ErrMalformedReference:         "MalformedReference",
	syntax.ErrUndefinedReference:         "UndefinedReference",
	syntax.ErrIllegalEndEscape:           "IllegalEndEscape",
	syntax.ErrMalformedSlashP:            "MalformedSlashP",
	syntax.ErrIncompleteSlashP:           "IncompleteSlashP",
	syntax.ErrUnknownSlashP:              "UnknownSlashP",
	syntax.ErrUnrecognizedEscape:         "UnrecognizedEscape",
	syntax.ErrMissingControl:             "MissingControl",
	syntax.ErrUnrecognizedControl:        "UnrecognizedControl",
	syntax.ErrTooFewHex:                  "TooFewHex",
	syntax.ErrInvalidHex:                 "InvalidHex",
	syntax.ErrMalformedNameRef:           "MalformedNameRef",
	syntax.ErrBadClassInCharRange:        "BadClassInCharRange",
	syntax.ErrUnterminatedBracket:        "UnterminatedBracket",
	syntax.ErrSubtractionMustBeLast:      "SubtractionMustBeLast",
	syntax.ErrReversedCharRange:          "ReversedCharRange",
}

var plainErrors = map[string]string{
	"startAt must be less than the length of the input string":            "StartAtTooLarge",
	"startAt must align to the start of a valid rune in the input string": "StartAtNotAligned",
	"Count too small": "CountTooSmall",
}

func errClass(err error) map[string]any {
	if se, ok := err.(*syntax.Error); ok {
		name, ok := errNames[se.Code]
		if !ok {
			panic(fmt.Sprintf("unknown error code %q", string(se.Code)))
		}
		args := []any{}
		for _, a := range se.Args {
			switch v := a.(type) {
			case int:
				args = append(args, map[string]any{"int": v})
			case string:
				args = append(args, map[string]any{"str": v})
			case rune:
				args = append(args, map[string]any{"rune": v})
			default:
				panic(fmt.Sprintf("unknown error argument %T", a))
			}
		}
		return map[string]any{"code": name, "args": args}
	}
	name, ok := plainErrors[err.Error()]
	if !ok {
		panic(fmt.Sprintf("unknown error %q", err.Error()))
	}
	return map[string]any{"code": name}
}

func putText(m map[string]any, field, s string) {
	if utf8.ValidString(s) {
		m[field] = s
	} else {
		m[field+"_hex"] = hex.EncodeToString([]byte(s))
	}
}

func decodeHex(s string) (int, bool) {
	v := 0
	for i := 0; i < len(s); i++ {
		c := s[i]
		var n byte
		switch {
		case '0' <= c && c <= '9':
			n = c - '0'
		case 'a' <= c && c <= 'f':
			n = c - 'a' + 10
		case 'A' <= c && c <= 'F':
			n = c - 'A' + 10
		default:
			return 0, false
		}
		v = v*16 + int(n)
	}
	return v, true
}

func writeHex4(b *strings.Builder, i int) {
	const digits = "0123456789abcdef"
	b.WriteByte(digits[i>>12])
	b.WriteByte(digits[(i>>8)&0xF])
	b.WriteByte(digits[(i>>4)&0xF])
	b.WriteByte(digits[i&0xF])
}

func convertRegexpToUnicode(patternStr string) string {
	var sb strings.Builder
	pos := 0
	for i := 0; i < len(patternStr)-11; {
		r, size := utf8.DecodeRuneInString(patternStr[i:])
		if r == '\\' {
			i++
			if patternStr[i] == 'u' && patternStr[i+5] == '\\' && patternStr[i+6] == 'u' {
				if first, ok := decodeHex(patternStr[i+1 : i+5]); ok {
					if first >= 0xD800 && first <= 0xDBFF {
						if second, ok := decodeHex(patternStr[i+7 : i+11]); ok {
							if second >= 0xDC00 && second <= 0xDFFF {
								r = utf16.DecodeRune(rune(first), rune(second))
								sb.WriteString(patternStr[pos : i-1])
								sb.WriteRune(r)
								i += 11
								pos = i
								continue
							}
						}
					}
				}
			}
			i++
		} else {
			i += size
		}
	}
	if pos > 0 {
		sb.WriteString(patternStr[pos:])
		return sb.String()
	}
	return patternStr
}

func convertRegexpToUtf16(patternStr string) string {
	var sb strings.Builder
	pos := 0
	var prevRune rune
	for i := 0; i < len(patternStr); {
		r, size := utf8.DecodeRuneInString(patternStr[i:])
		if r > 0xFFFF {
			sb.WriteString(patternStr[pos:i])
			if prevRune == '\\' {
				sb.WriteRune('\\')
			}
			first, second := utf16.EncodeRune(r)
			sb.WriteString(`\u`)
			writeHex4(&sb, int(first))
			sb.WriteString(`\u`)
			writeHex4(&sb, int(second))
			pos = i + size
		}
		i += size
		prevRune = r
	}
	if pos > 0 {
		sb.WriteString(patternStr[pos:])
		return sb.String()
	}
	return patternStr
}

func gojaOptions(flags string) regexp2.RegexOptions {
	opts := regexp2.RegexOptions(regexp2.ECMAScript)
	if strings.Contains(flags, "m") {
		opts |= regexp2.Multiline
	}
	if strings.Contains(flags, "s") {
		opts |= regexp2.Singleline
	}
	if strings.Contains(flags, "i") {
		opts |= regexp2.IgnoreCase
	}
	if strings.Contains(flags, "u") {
		opts |= regexp2.Unicode
	}
	return opts
}

type part struct {
	units []uint16
	raw   []rune
	times int
}

func raw(runes ...rune) part {
	return part{raw: runes, times: 1}
}

func s(text string) part {
	return part{units: utf16.Encode([]rune(text)), times: 1}
}

func u(units ...uint16) part {
	return part{units: units, times: 1}
}

func rep(text string, n int) part {
	return part{units: utf16.Encode([]rune(text)), times: n}
}

func runesOf(units []uint16, unicode bool) []rune {
	if !unicode {
		r := make([]rune, len(units))
		for i, c := range units {
			r[i] = rune(c)
		}
		return r
	}
	var r []rune
	for i := 0; i < len(units); i++ {
		c := units[i]
		if c >= 0xD800 && c <= 0xDBFF && i+1 < len(units) && units[i+1] >= 0xDC00 && units[i+1] <= 0xDFFF {
			r = append(r, utf16.DecodeRune(rune(c), rune(units[i+1])))
			i++
			continue
		}
		r = append(r, rune(c))
	}
	return r
}

type input struct {
	parts  []part
	starts []int
}

func in(parts ...part) input {
	return input{parts: parts}
}

func at(starts []int, parts ...part) input {
	return input{parts: parts, starts: starts}
}

type jsCase struct {
	label  string
	source string
	flags  string
	inputs []input
}

func capRec(c regexp2.Capture) []int {
	return []int{c.Index, c.Length}
}

func groupRec(g regexp2.Group) map[string]any {
	caps := [][]int{}
	for _, c := range g.Captures {
		caps = append(caps, capRec(c))
	}
	return map[string]any{"n": g.Name, "i": g.Index, "l": g.Length, "c": caps}
}

func matchRec(m *regexp2.Match) map[string]any {
	groups := []any{}
	for _, g := range m.Groups() {
		groups = append(groups, groupRec(g))
	}
	return map[string]any{"i": m.Index, "l": m.Length, "n": m.GroupCount(), "g": groups}
}

const maxMatches = 20

func findAll(re *regexp2.Regexp, runes []rune, start int, limit int) (rec map[string]any) {
	matches := []any{}
	rec = map[string]any{"start": start}
	defer func() {
		if p := recover(); p != nil {
			rec["matches"] = matches
			rec["panic"] = true
		}
	}()
	m, err := re.FindRunesMatchStartingAt(runes, start)
	for err == nil && m != nil && len(matches) < limit {
		matches = append(matches, matchRec(m))
		m, err = re.FindNextMatch(m)
	}
	if err != nil {
		rec["error"] = errClass(err)
	}
	rec["matches"] = matches
	rec["more"] = m != nil
	return rec
}

func quick(re *regexp2.Regexp, runes []rune) (rec any) {
	defer func() {
		if p := recover(); p != nil {
			rec = "panic"
		}
	}()
	ok, err := re.MatchRunes(runes)
	if err != nil {
		return errClass(err)
	}
	return ok
}

func regexpInfo(re *regexp2.Regexp) map[string]any {
	names := re.GetGroupNames()
	nums := re.GetGroupNumbers()
	maxNum := 0
	for _, n := range nums {
		if n > maxNum {
			maxNum = n
		}
	}
	byNumber := []string{}
	for i := -1; i <= maxNum+2; i++ {
		byNumber = append(byNumber, re.GroupNameFromNumber(i))
	}
	probe := append([]string{}, names...)
	probe = append(probe, "", "x", "99", "-1", "00", "1a")
	byName := []any{}
	for _, n := range probe {
		byName = append(byName, []any{n, re.GroupNumberFromName(n)})
	}
	return map[string]any{"names": names, "numbers": nums, "name_of": byNumber, "number_of": byName}
}

func codeRec(pattern string, opts regexp2.RegexOptions) (rec map[string]any) {
	rec = map[string]any{}
	defer func() {
		if p := recover(); p != nil {
			rec = map[string]any{"panic": true}
		}
	}()
	tree, err := syntax.Parse(pattern, syntax.RegexOptions(opts))
	if err != nil {
		return map[string]any{"error": errClass(err)}
	}
	code, err := syntax.Write(tree)
	if err != nil {
		panic(err)
	}
	stringsTable := [][]rune{}
	for _, s := range code.Strings {
		stringsTable = append(stringsTable, s)
	}
	sets := []string{}
	for _, set := range code.Sets {
		sets = append(sets, set.String())
	}
	var caps any
	if code.Caps != nil {
		keys := []int{}
		for k := range code.Caps {
			keys = append(keys, k)
		}
		sort.Ints(keys)
		pairs := [][]int{}
		for _, k := range keys {
			pairs = append(pairs, []int{k, code.Caps[k]})
		}
		caps = pairs
	}
	var capnames any
	if tree.Capnames != nil {
		keys := []string{}
		for k := range tree.Capnames {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		pairs := [][]any{}
		for _, k := range keys {
			pairs = append(pairs, []any{k, tree.Capnames[k]})
		}
		capnames = pairs
	}
	var caplist any
	if tree.Caplist != nil {
		caplist = tree.Caplist
	}
	var fc any
	if code.FcPrefix != nil {
		fc = map[string]any{"set": code.FcPrefix.PrefixSet.String(), "ci": code.FcPrefix.CaseInsensitive}
	}
	var bm any
	if code.BmPrefix != nil {
		bm = code.BmPrefix.Dump("")
	}
	rec["codes"] = code.Codes
	rec["strings"] = stringsTable
	rec["sets"] = sets
	rec["trackcount"] = code.TrackCount
	rec["capsize"] = code.Capsize
	rec["caps"] = caps
	rec["fc"] = fc
	rec["bm"] = bm
	rec["anchors"] = int(code.Anchors)
	rec["rtl"] = code.RightToLeft
	rec["capnames"] = capnames
	rec["caplist"] = caplist
	rec["tree"] = tree.Dump()
	rec["dump"] = code.Dump()
	return rec
}

func compileGuarded(pattern string, opts regexp2.RegexOptions) (re *regexp2.Regexp, err error, panicked bool) {
	defer func() {
		if p := recover(); p != nil {
			re, err, panicked = nil, nil, true
		}
	}()
	re, err = regexp2.Compile(pattern, opts)
	return re, err, false
}

func partsRec(parts []part, unicode bool) ([]rune, []any) {
	var all []rune
	recs := []any{}
	for _, p := range parts {
		r := p.raw
		if r == nil {
			r = runesOf(p.units, unicode)
		}
		if r == nil {
			r = []rune{}
		}
		recs = append(recs, map[string]any{"r": r, "n": p.times})
		for i := 0; i < p.times; i++ {
			all = append(all, r...)
		}
	}
	if all == nil {
		all = []rune{}
	}
	return all, recs
}

func genGoja(out string) error {
	var lines goldenio.Lines
	for _, c := range gojaCases() {
		unicode := strings.Contains(c.flags, "u")
		var pattern string
		if unicode {
			pattern = convertRegexpToUnicode(c.source)
		} else {
			pattern = convertRegexpToUtf16(c.source)
		}
		opts := gojaOptions(c.flags)
		rec := map[string]any{"label": c.label, "flags": c.flags, "options": int(opts)}
		putText(rec, "pattern", pattern)
		re, err, panicked := compileGuarded(pattern, opts)
		switch {
		case panicked:
			rec["compile"] = "panic"
		case err != nil:
			rec["compile"] = errClass(err)
		default:
			rec["compile"] = "ok"
			rec["code"] = codeRec(pattern, opts)
			rec["info"] = regexpInfo(re)
			runs := []any{}
			for _, inp := range c.inputs {
				runes, parts := partsRec(inp.parts, unicode)
				starts := inp.starts
				if starts == nil {
					starts = []int{0}
				}
				finds := []any{}
				for _, st := range starts {
					finds = append(finds, findAll(re, runes, st, maxMatches))
				}
				runs = append(runs, map[string]any{"input": parts, "finds": finds, "quick": quick(re, runes)})
			}
			rec["runs"] = runs
		}
		lines.Add(rec)
	}
	fmt.Fprintf(os.Stderr, "talos.jsonl: %d lines\n", lines.Len())
	return lines.Write(out, "talos.jsonl")
}

var corpusOptions = []regexp2.RegexOptions{
	regexp2.None,
	regexp2.ECMAScript,
	regexp2.ECMAScript | regexp2.IgnoreCase | regexp2.Multiline | regexp2.Singleline | regexp2.Unicode,
	regexp2.RightToLeft | regexp2.IgnoreCase | regexp2.IgnorePatternWhitespace | regexp2.ExplicitCapture,
}

var corpusInputs = []string{"aB1 _-\néx(", "zzİK\U0001F600a"}

func compactMatch(m *regexp2.Match) []any {
	groups := []any{}
	for _, g := range m.Groups() {
		caps := []int{}
		for _, c := range g.Captures {
			caps = append(caps, c.Index, c.Length)
		}
		groups = append(groups, []any{g.Index, g.Length, caps})
	}
	return []any{m.Index, m.Length, groups}
}

func findCompact(re *regexp2.Regexp, runes []rune, limit int) (rec any) {
	matches := []any{}
	defer func() {
		if p := recover(); p != nil {
			rec = map[string]any{"panic": true, "matches": matches}
		}
	}()
	m, err := re.FindRunesMatchStartingAt(runes, 0)
	for err == nil && m != nil && len(matches) < limit {
		matches = append(matches, compactMatch(m))
		m, err = re.FindNextMatch(m)
	}
	if err != nil {
		return nil
	}
	return matches
}

func corpusDir() (string, error) {
	outb, err := exec.Command("go", "env", "GOMODCACHE").Output()
	if err != nil {
		return "", err
	}
	dir := filepath.Join(strings.TrimSpace(string(outb)), filepath.FromSlash(module), "syntax", "workdir", "corpus")
	if _, err := os.Stat(dir); err != nil {
		return "", err
	}
	return dir, nil
}

func genCorpus(out string) error {
	dir, err := corpusDir()
	if err != nil {
		return err
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return err
	}
	names := []string{}
	for _, e := range entries {
		if !e.IsDir() {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)
	var lines goldenio.Lines
	timeouts := 0
	for _, name := range names {
		data, err := os.ReadFile(filepath.Join(dir, name))
		if err != nil {
			return err
		}
		pattern := string(data)
		for _, opts := range corpusOptions {
			rec := map[string]any{"name": name, "options": int(opts)}
			putText(rec, "pattern", pattern)
			code := codeRec(pattern, opts)
			delete(code, "tree")
			delete(code, "dump")
			rec["code"] = code
			if code["panic"] == nil && code["error"] == nil {
				re, err, panicked := compileGuarded(pattern, opts)
				if panicked || err != nil {
					return fmt.Errorf("corpus %s: compile differs from parse", name)
				}
				re.MatchTimeout = 2 * time.Second
				runs := []any{}
				probe := append([]string{}, corpusInputs...)
				probe = append(probe, pattern)
				for _, text := range probe {
					runes := []rune(text)
					if len(runes) > 40 {
						runes = runes[:40]
					}
					f := findCompact(re, runes, 3)
					if f == nil {
						timeouts++
					}
					runs = append(runs, f)
				}
				rec["runs"] = runs
			}
			lines.Add(rec)
		}
	}
	fmt.Fprintf(os.Stderr, "corpus.jsonl: %d lines, %d runs dropped by timeout\n", lines.Len(), timeouts)
	return lines.Write(out, "corpus.jsonl")
}

type replaceCase struct {
	pattern string
	opts    regexp2.RegexOptions
	input   string
	repl    string
	startAt int
	count   int
}

func genReplace(out string) error {
	cases := []replaceCase{
		{`test`, 0, "this is a test", "unit", -1, -1},
		{`(a)(b)?`, regexp2.ECMAScript, "xaab", "[$1|$2|$&|$$|$`|$'|$+|$_]", -1, -1},
		{`(a)(b)?`, 0, "xaab", "[$1|$2|$&|$$|$`|$'|$+|$_]", -1, -1},
		{`(?<x>a)`, regexp2.ECMAScript, "aa", "${x}-$<x>-${1}-$1-${2}-$2", -1, -1},
		{`(a)`, regexp2.ECMAScript, "aa", "$10$01$1$", -1, -1},
		{`(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)`, regexp2.ECMAScript, "abcdefghijk", "$11-$110-$1a-$012", -1, -1},
		{`(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)`, 0, "abcdefghijk", "$11-$110-$1a-$012-${11}", -1, -1},
		{`\d`, regexp2.RightToLeft, "0123456789foo4567890foo", "#", -1, 7},
		{`\d`, regexp2.RightToLeft, "0123456789foo4567890foo", "<$&$`>", 15, 3},
		{`(?<x>\d)`, regexp2.RightToLeft, "a1b2c3", "[${x}$+$_]", -1, -1},
		{`a`, 0, "aaaaa", "b", 3, 2},
		{`a`, 0, "aaaaa", "b", 0, 0},
		{`a`, 0, "aaaaa", "b", 0, -2},
		{`a`, 0, "aaaaa", "b", 6, -1},
		{`x*`, 0, "abc", "-", -1, -1},
		{`x*`, regexp2.RightToLeft, "abc", "-", -1, -1},
		{`é`, 0, "aébé", "E", 1, -1},
		{`é`, 0, "aébé", "E", 2, -1},
		{`a`, 0, "this is a test", `$5000000000`, -1, -1},
		{`(a)`, 0, "a", `${5000000000}`, -1, -1},
		{`(a)`, regexp2.ECMAScript, "a", `$5000000000`, -1, -1},
		{`(?<n>a)|(?<m>b)`, 0, "ab", "${n}${m}$1$2${3}", -1, -1},
		{`(?<2>a)(b)`, 0, "ab", "$1|$2|$3|${2}", -1, -1},
		{`(?<5>a)(b)`, 0, "ab", "$1|$2|$5|$6|$+", -1, -1},
		{`\w+`, regexp2.IgnoreCase, "Hello World", "<$0>", -1, -1},
		{`(.)`, regexp2.ECMAScript | regexp2.Unicode, "a\U0001F600b", "[$1]", -1, -1},
		{`$`, 0, "abc\n", "!", -1, -1},
		{`^`, regexp2.Multiline, "a\nb\nc", ">", -1, -1},
		{`b`, 0, "abc", "$", -1, -1},
		{`b`, 0, "abc", "${", -1, -1},
		{`b`, 0, "abc", "${0", -1, -1},
		{`b`, 0, "abc", "${0}${x}", -1, -1},
	}
	var lines goldenio.Lines
	for _, c := range cases {
		rec := map[string]any{"options": int(c.opts), "start_at": c.startAt, "count": c.count}
		putText(rec, "pattern", c.pattern)
		putText(rec, "input", c.input)
		putText(rec, "replacement", c.repl)
		re := regexp2.MustCompile(c.pattern, c.opts)
		res, err := re.Replace(c.input, c.repl, c.startAt, c.count)
		if err != nil {
			rec["error"] = errClass(err)
		} else {
			putText(rec, "output", res)
		}
		evaluated, err := re.ReplaceFunc(c.input, func(m regexp2.Match) string {
			return fmt.Sprintf("<%d,%d,%d>", m.Index, m.Length, m.GroupCount())
		}, c.startAt, c.count)
		if err != nil {
			rec["func_error"] = errClass(err)
		} else {
			putText(rec, "func_output", evaluated)
		}
		lines.Add(rec)
	}
	fmt.Fprintf(os.Stderr, "replace.jsonl: %d lines\n", lines.Len())
	return lines.Write(out, "replace.jsonl")
}

func genEscape(out string) error {
	inputs := []string{
		"", "abc", "#$^*+(){}<>\\|. ", "[a-z]?", "\a\f\n\r\t\v\x00\x01\x1b\x7f",
		"\u0085\u00a0\u00ad\u00ff\u0100\u0378\u2028\u2029\u200b\ufeff\uffff",
		"\U0001F600\U000E0001\U0010FFFF", "tab\there", "a\x80b", "\xff\xfe",
	}
	for r := rune(0); r < 0x300; r++ {
		inputs = append(inputs, string(r))
	}
	unescapes := []string{
		`a\x20b`, `Aé`, `\x{1F600}`, `\cA\ca\c[`, `\0\07\101\400`, `\e\a\b\f\n\r\t\v`,
		`\`, `abc\`, `\x`, `\x2`, `\x2R`, `\u12`, `\x{}`, `\x{110000}`, `\x{12`, `\c`, `\c1`, `\q`, `\_`, `\.`,
		`no escapes`, `\\\\`, `a\\b\\`,
	}
	var lines goldenio.Lines
	for _, in := range inputs {
		rec := map[string]any{"kind": "escape"}
		putText(rec, "input", in)
		esc := regexp2.Escape(in)
		putText(rec, "output", esc)
		back, err := regexp2.Unescape(esc)
		if err != nil {
			rec["error"] = errClass(err)
		} else {
			putText(rec, "unescaped", back)
		}
		lines.Add(rec)
	}
	for _, in := range unescapes {
		rec := map[string]any{"kind": "unescape"}
		putText(rec, "input", in)
		back, err := regexp2.Unescape(in)
		if err != nil {
			rec["error"] = errClass(err)
		} else {
			putText(rec, "output", back)
		}
		lines.Add(rec)
	}
	fmt.Fprintf(os.Stderr, "escape.jsonl: %d lines\n", lines.Len())
	return lines.Write(out, "escape.jsonl")
}

func genStrings(out string) error {
	type sc struct {
		pattern string
		opts    regexp2.RegexOptions
		input   string
		starts  []int
	}
	cases := []sc{
		{`.`, 0, "aé😀b", []int{-1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10}},
		{`.`, regexp2.RightToLeft, "aé😀b", []int{-1, 0, 1, 3, 7, 8, 9}},
		{`\w+`, 0, "\xffab\xe9", []int{-1, 0, 1, 2, 3, 4, 5}},
		{`(?:)`, 0, "t", []int{0, 1, 2}},
		{`b`, regexp2.ECMAScript, "abc", []int{0, 1, 2, 3, 4}},
	}
	var lines goldenio.Lines
	for _, c := range cases {
		re := regexp2.MustCompile(c.pattern, c.opts)
		for _, st := range c.starts {
			rec := map[string]any{"options": int(c.opts), "start_at": st}
			putText(rec, "pattern", c.pattern)
			putText(rec, "input", c.input)
			m, err := re.FindStringMatchStartingAt(c.input, st)
			if err != nil {
				rec["error"] = errClass(err)
			} else if m != nil {
				rec["match"] = matchRec(m)
				putText(rec, "text", m.String())
			} else {
				rec["match"] = nil
			}
			lines.Add(rec)
		}
		rec := map[string]any{"options": int(c.opts), "start_at": nil}
		putText(rec, "pattern", c.pattern)
		putText(rec, "input", c.input)
		m, err := re.FindStringMatch(c.input)
		if err != nil {
			rec["error"] = errClass(err)
		} else if m != nil {
			rec["match"] = matchRec(m)
			putText(rec, "text", m.String())
		} else {
			rec["match"] = nil
		}
		ok, err := re.MatchString(c.input)
		if err != nil {
			return err
		}
		rec["quick"] = ok
		lines.Add(rec)
	}
	fmt.Fprintf(os.Stderr, "strings.jsonl: %d lines\n", lines.Len())
	return lines.Write(out, "strings.jsonl")
}
