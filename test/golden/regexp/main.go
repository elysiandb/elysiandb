package main

import (
	"bufio"
	"compress/bzip2"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"math/rand"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"regexp/syntax"
	"runtime"
	"slices"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

const maxInline = 2048

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("regexp", run)
}

func run(out string) error {
	testdata := filepath.Join(runtime.GOROOT(), "src", "regexp", "testdata")
	if _, err := os.Stat(testdata); err != nil {
		return err
	}
	g := &gen{out: out, seen: map[string]bool{}, parts: map[string][][2]any{}}
	steps := []func() error{
		func() error { return g.re2Search(filepath.Join(testdata, "re2-search.txt")) },
		func() error { return g.re2Exhaustive(filepath.Join(testdata, "re2-exhaustive.txt.bz2")) },
		func() error { return g.fowler(testdata) },
		g.corpus,
		g.syntaxCorpus,
		g.fuzz,
	}
	for _, step := range steps {
		if err := step(); err != nil {
			return err
		}
	}
	return g.compiles.Write(out, "compile.jsonl")
}

type gen struct {
	out      string
	compiles goldenio.Lines
	seen     map[string]bool
	parts    map[string][][2]any
}

type part struct {
	unit  string
	count int
}

func (g *gen) build(ps ...part) string {
	var b strings.Builder
	desc := make([][2]any, len(ps))
	for i, p := range ps {
		b.WriteString(strings.Repeat(p.unit, p.count))
		desc[i] = [2]any{p.unit, p.count}
	}
	s := b.String()
	g.parts[s] = desc
	return s
}

func (g *gen) patternField(m map[string]any, pattern string) error {
	if len(pattern) <= maxInline {
		textOrHex(m, "pattern", pattern)
		return nil
	}
	desc, ok := g.parts[pattern]
	if !ok {
		return fmt.Errorf("long pattern without parts: %.40q", pattern)
	}
	m["pattern_parts"] = desc
	return nil
}

func textOrHex(m map[string]any, field string, s string) {
	if utf8.ValidString(s) {
		m[field] = s
	} else {
		m[field+"_hex"] = hex.EncodeToString([]byte(s))
	}
}

func bigText(m map[string]any, field string, s string) {
	if len(s) <= maxInline {
		textOrHex(m, field, s)
	} else {
		m[field+"_fnv"] = fnv(s)
	}
}

func fnv(s string) string {
	h := uint64(14695981039346656037)
	for i := 0; i < len(s); i++ {
		h ^= uint64(s[i])
		h *= 1099511628211
	}
	return fmt.Sprintf("%016x", h)
}

func errorClass(err error) (string, string, error) {
	var se *syntax.Error
	if !errors.As(err, &se) {
		return "", "", fmt.Errorf("unexpected error type %T", err)
	}
	classes := map[syntax.ErrorCode]string{
		syntax.ErrInternalError:         "internal_error",
		syntax.ErrInvalidCharClass:      "invalid_char_class",
		syntax.ErrInvalidCharRange:      "invalid_char_range",
		syntax.ErrInvalidEscape:         "invalid_escape",
		syntax.ErrInvalidNamedCapture:   "invalid_named_capture",
		syntax.ErrInvalidPerlOp:         "invalid_perl_op",
		syntax.ErrInvalidRepeatOp:       "invalid_repeat_op",
		syntax.ErrInvalidRepeatSize:     "invalid_repeat_size",
		syntax.ErrInvalidUTF8:           "invalid_utf8",
		syntax.ErrMissingBracket:        "missing_bracket",
		syntax.ErrMissingParen:          "missing_paren",
		syntax.ErrMissingRepeatArgument: "missing_repeat_argument",
		syntax.ErrTrailingBackslash:     "trailing_backslash",
		syntax.ErrUnexpectedParen:       "unexpected_paren",
		syntax.ErrNestingDepth:          "nesting_depth",
		syntax.ErrLarge:                 "large",
	}
	class, ok := classes[se.Code]
	if !ok {
		return "", "", fmt.Errorf("unknown error code %q", string(se.Code))
	}
	return class, se.Expr, nil
}

func setError(m map[string]any, err error) error {
	class, expr, e := errorClass(err)
	if e != nil {
		return e
	}
	m["error"] = class
	bigText(m, "expr", expr)
	return nil
}

func ints(a []int) any {
	if a == nil {
		return nil
	}
	return a
}

func intss(a [][]int) any {
	if a == nil {
		return nil
	}
	return a
}

func resText(a []int) string {
	if a == nil {
		return "-;"
	}
	var b strings.Builder
	for i, v := range a {
		if i > 0 {
			b.WriteByte(' ')
		}
		b.WriteString(strconv.Itoa(v))
	}
	b.WriteByte(';')
	return b.String()
}

func allText(a [][]int) string {
	if a == nil {
		return "-;"
	}
	var b strings.Builder
	for i, m := range a {
		if i > 0 {
			b.WriteByte(',')
		}
		for j, v := range m {
			if j > 0 {
				b.WriteByte(' ')
			}
			b.WriteString(strconv.Itoa(v))
		}
	}
	b.WriteByte(';')
	return b.String()
}

func boolText(v bool) string {
	if v {
		return "1"
	}
	return "0"
}

func field(re *regexp.Regexp, name string) reflect.Value {
	return reflect.ValueOf(re).Elem().FieldByName(name)
}

type sink struct {
	buf []byte
	n   int
	h   uint64
}

func newSink() *sink {
	return &sink{h: 14695981039346656037}
}

func (s *sink) WriteString(p string) {
	for i := 0; i < len(p); i++ {
		s.h ^= uint64(p[i])
		s.h *= 1099511628211
	}
	if s.n <= maxInline {
		s.buf = append(s.buf, p...)
	}
	s.n += len(p)
}

func (s *sink) set(m map[string]any, key string) {
	if s.n <= maxInline {
		textOrHex(m, key, string(s.buf))
	} else {
		m[key+"_fnv"] = fmt.Sprintf("%016x", s.h)
	}
}

func (s *sink) repr() string {
	if s.n <= maxInline {
		return string(s.buf)
	}
	return fmt.Sprintf("#%016x", s.h)
}

func dumpInsts(w *sink, p reflect.Value, next bool) {
	w.WriteString("start " + strconv.FormatInt(p.FieldByName("Start").Int(), 10) + " numcap " + strconv.FormatInt(p.FieldByName("NumCap").Int(), 10) + "\n")
	insts := p.FieldByName("Inst")
	for i := 0; i < insts.Len(); i++ {
		in := insts.Index(i)
		w.WriteString(strconv.Itoa(i) + " " + strconv.FormatUint(in.FieldByName("Op").Uint(), 10) + " " + strconv.FormatUint(in.FieldByName("Out").Uint(), 10) + " " + strconv.FormatUint(in.FieldByName("Arg").Uint(), 10) + " [")
		runes := in.FieldByName("Rune")
		for j := 0; j < runes.Len(); j++ {
			if j > 0 {
				w.WriteString(" ")
			}
			w.WriteString(strconv.FormatInt(runes.Index(j).Int(), 10))
		}
		w.WriteString("]")
		if next {
			w.WriteString(" [")
			nx := in.FieldByName("Next")
			for j := 0; j < nx.Len(); j++ {
				if j > 0 {
					w.WriteString(" ")
				}
				w.WriteString(strconv.FormatUint(nx.Index(j).Uint(), 10))
			}
			w.WriteString("]")
		}
		w.WriteString("\n")
	}
}

func progSize(re *regexp.Regexp) (int, int) {
	insts := field(re, "prog").Elem().FieldByName("Inst")
	runes := 0
	for i := 0; i < insts.Len(); i++ {
		runes += insts.Index(i).FieldByName("Rune").Len()
	}
	return insts.Len(), runes
}

func hugeProg(re *regexp.Regexp) bool {
	insts, runes := progSize(re)
	return insts > 100000 || runes > 1000000
}

func onePassDump(re *regexp.Regexp) (*sink, bool) {
	v := field(re, "onepass")
	if v.IsNil() {
		return newSink(), false
	}
	w := newSink()
	dumpInsts(w, v.Elem(), true)
	return w, true
}

func progDump(re *regexp.Regexp) *sink {
	w := newSink()
	dumpInsts(w, field(re, "prog").Elem(), false)
	return w
}

func compileSummary(re *regexp.Regexp, err error) (string, error) {
	if err != nil {
		class, expr, e := errorClass(err)
		if e != nil {
			return "", e
		}
		return "error " + class + " " + hex.EncodeToString([]byte(expr)), nil
	}
	op, _ := onePassDump(re)
	prefix, complete := re.LiteralPrefix()
	parts := []string{
		progDump(re).repr(),
		op.repr(),
		prefix,
		boolText(complete),
		strconv.FormatUint(field(re, "cond").Uint(), 10),
		strconv.FormatInt(field(re, "minInputLen").Int(), 10),
		strconv.FormatInt(field(re, "maxBitStateLen").Int(), 10),
		strconv.Itoa(re.NumSubexp()),
		strconv.FormatInt(field(re, "matchcap").Int(), 10),
		strconv.FormatInt(field(re, "prefixRune").Int(), 10),
		strconv.FormatUint(field(re, "prefixEnd").Uint(), 10),
		strconv.FormatInt(field(re, "mpool").Int(), 10),
		strings.Join(re.SubexpNames(), ","),
	}
	return strings.Join(parts, "\x00"), nil
}

func (g *gen) compileRecord(pattern string, posix bool) error {
	key := strconv.FormatBool(posix) + "\x00" + pattern
	if g.seen[key] {
		return nil
	}
	g.seen[key] = true
	m := map[string]any{"posix": posix}
	if err := g.patternField(m, pattern); err != nil {
		return err
	}
	var re *regexp.Regexp
	var err error
	if posix {
		re, err = regexp.CompilePOSIX(pattern)
	} else {
		re, err = regexp.Compile(pattern)
	}
	if err != nil {
		if e := setError(m, err); e != nil {
			return e
		}
		g.compiles.Add(m)
		return nil
	}
	prefix, complete := re.LiteralPrefix()
	m["string"] = re.String() == pattern
	m["num_subexp"] = re.NumSubexp()
	m["names"] = re.SubexpNames()
	textOrHex(m, "prefix", prefix)
	m["complete"] = complete
	m["prefix_rune"] = field(re, "prefixRune").Int()
	m["prefix_end"] = field(re, "prefixEnd").Uint()
	m["max_bitstate_len"] = field(re, "maxBitStateLen").Int()
	m["min_input_len"] = field(re, "minInputLen").Int()
	m["cond"] = field(re, "cond").Uint()
	m["matchcap"] = field(re, "matchcap").Int()
	m["mpool"] = field(re, "mpool").Int()
	insts, runes := progSize(re)
	m["insts"] = insts
	m["runes"] = runes
	if hugeProg(re) {
		g.compiles.Add(m)
		return nil
	}
	progDump(re).set(m, "prog")
	if op, ok := onePassDump(re); ok {
		op.set(m, "onepass")
	} else {
		m["onepass"] = nil
	}
	g.compiles.Add(m)
	return nil
}

func (g *gen) re2Search(file string) error {
	var lines goldenio.Lines
	err := readRE2(file, func(q string, re, refull *regexp.Regexp, err error, texts []string) error {
		if e := g.compileRecord(q, false); e != nil {
			return e
		}
		if err != nil {
			m := map[string]any{}
			textOrHex(m, "pattern", q)
			if e := setError(m, err); e != nil {
				return e
			}
			lines.Add(m)
			return nil
		}
		if e := g.compileRecord(`\A(?:`+q+`)\z`, false); e != nil {
			return e
		}
		for _, text := range texts {
			res, match := runRE2(re, refull, text)
			m := map[string]any{}
			textOrHex(m, "pattern", q)
			textOrHex(m, "text", text)
			m["full"] = ints(res[0])
			m["partial"] = ints(res[1])
			m["full_longest"] = ints(res[2])
			m["partial_longest"] = ints(res[3])
			m["match"] = match[:]
			lines.Add(m)
		}
		return nil
	})
	if err != nil {
		return err
	}
	return lines.Write(g.out, "re2-search.jsonl")
}

func (g *gen) re2Exhaustive(file string) error {
	var lines goldenio.Lines
	var current []string
	err := readRE2(file, func(q string, re, refull *regexp.Regexp, err error, texts []string) error {
		if !slices.Equal(texts, current) {
			current = slices.Clone(texts)
			m := map[string]any{}
			valid := true
			for _, s := range texts {
				if !utf8.ValidString(s) {
					valid = false
				}
			}
			if valid {
				m["strings"] = texts
			} else {
				hs := make([]string, len(texts))
				for i, s := range texts {
					hs[i] = hex.EncodeToString([]byte(s))
				}
				m["strings_hex"] = hs
			}
			lines.Add(m)
		}
		m := map[string]any{}
		textOrHex(m, "pattern", q)
		if err != nil {
			if e := setError(m, err); e != nil {
				return e
			}
			lines.Add(m)
			return nil
		}
		var b strings.Builder
		s1, _ := compileSummary(re, nil)
		s2, _ := compileSummary(refull, nil)
		b.WriteString(s1)
		b.WriteByte('\n')
		b.WriteString(s2)
		b.WriteByte('\n')
		for _, text := range texts {
			res, match := runRE2(re, refull, text)
			for _, r := range res {
				b.WriteString(resText(r))
			}
			for _, v := range match {
				b.WriteString(boolText(v))
			}
			b.WriteByte('\n')
		}
		m["cases"] = len(texts)
		m["fnv"] = fnv(b.String())
		lines.Add(m)
		return nil
	})
	if err != nil {
		return err
	}
	return lines.Write(g.out, "re2-exhaustive.jsonl")
}

func runRE2(re, refull *regexp.Regexp, text string) ([4][]int, [4]bool) {
	relong := re.Copy()
	relong.Longest()
	refulllong := refull.Copy()
	refulllong.Longest()
	return [4][]int{
			refull.FindStringSubmatchIndex(text),
			re.FindStringSubmatchIndex(text),
			refulllong.FindStringSubmatchIndex(text),
			relong.FindStringSubmatchIndex(text),
		}, [4]bool{
			refull.MatchString(text),
			re.MatchString(text),
			refulllong.MatchString(text),
			relong.MatchString(text),
		}
}

func readRE2(file string, visit func(q string, re, refull *regexp.Regexp, err error, texts []string) error) error {
	f, err := os.Open(file)
	if err != nil {
		return err
	}
	defer f.Close()
	var txt io.Reader = f
	if strings.HasSuffix(file, ".bz2") {
		txt = bzip2.NewReader(f)
	}
	scanner := bufio.NewScanner(txt)
	var (
		str       []string
		inStrings bool
		pending   string
		results   int
		re        *regexp.Regexp
		refull    *regexp.Regexp
		cerr      error
		active    bool
	)
	flush := func() error {
		if !active {
			return nil
		}
		active = false
		if cerr == nil && results != len(str) {
			return fmt.Errorf("%s: %d results for %d strings at %q", file, results, len(str), pending)
		}
		return visit(pending, re, refull, cerr, str)
	}
	for lineno := 1; scanner.Scan(); lineno++ {
		line := scanner.Text()
		switch {
		case line == "":
			return fmt.Errorf("%s:%d: unexpected blank line", file, lineno)
		case line[0] == '#':
			continue
		case 'A' <= line[0] && line[0] <= 'Z':
			continue
		case line == "strings":
			if err := flush(); err != nil {
				return err
			}
			str = nil
			inStrings = true
		case line == "regexps":
			inStrings = false
		case line[0] == '"':
			q, err := strconv.Unquote(line)
			if err != nil {
				return fmt.Errorf("%s:%d: unquote: %v", file, lineno, err)
			}
			if inStrings {
				str = append(str, q)
				continue
			}
			if err := flush(); err != nil {
				return err
			}
			active = true
			pending = q
			results = 0
			refull = nil
			re, cerr = regexp.Compile(q)
			if cerr == nil {
				refull, err = regexp.Compile(`\A(?:` + q + `)\z`)
				if err != nil {
					return fmt.Errorf("%s:%d: compile full: %v", file, lineno, err)
				}
			}
		case line[0] == '-' || '0' <= line[0] && line[0] <= '9':
			results++
		default:
			return fmt.Errorf("%s:%d: out of sync: %s", file, lineno, line)
		}
	}
	if err := scanner.Err(); err != nil {
		return err
	}
	return flush()
}

func (g *gen) fowler(testdata string) error {
	files, err := filepath.Glob(filepath.Join(testdata, "*.dat"))
	if err != nil {
		return err
	}
	sort.Strings(files)
	var lines goldenio.Lines
	notab := regexp.MustCompilePOSIX(`[^\t]+`)
	for _, file := range files {
		if err := g.fowlerFile(file, notab, &lines); err != nil {
			return err
		}
	}
	return lines.Write(g.out, "fowler.jsonl")
}

func (g *gen) fowlerFile(file string, notab *regexp.Regexp, lines *goldenio.Lines) error {
	f, err := os.Open(file)
	if err != nil {
		return err
	}
	defer f.Close()
	b := bufio.NewReader(f)
	base := filepath.Base(file)
	lineno := 0
	lastRegexp := ""
Reading:
	for {
		lineno++
		line, err := b.ReadString('\n')
		if err != nil {
			if err != io.EOF {
				return err
			}
			break Reading
		}
		if line[0] == '#' || line[0] == '\n' {
			continue Reading
		}
		line = line[:len(line)-1]
		fields := notab.FindAllString(line, -1)
		for i, fl := range fields {
			if fl == "NULL" {
				fields[i] = ""
			}
			if fl == "NIL" {
				continue Reading
			}
		}
		if len(fields) == 0 {
			continue Reading
		}
		flag := fields[0]
		switch flag[0] {
		case '?', '&', '|', ';', '{', '}':
			flag = flag[1:]
			if flag == "" {
				continue Reading
			}
		case ':':
			var ok bool
			if _, flag, ok = strings.Cut(flag[1:], ":"); !ok {
				continue Reading
			}
		case 'C', 'N', 'T', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9':
			continue Reading
		}
		if len(fields) < 4 {
			return fmt.Errorf("%s:%d: too few fields", base, lineno)
		}
		if strings.Contains(flag, "$") {
			for k := 1; k <= 2; k++ {
				v, err := strconv.Unquote(`"` + fields[k] + `"`)
				if err != nil {
					return fmt.Errorf("%s:%d: cannot unquote %s", base, lineno, fields[k])
				}
				fields[k] = v
			}
		}
		if fields[1] == "SAME" {
			fields[1] = lastRegexp
		}
		lastRegexp = fields[1]
		text := fields[2]
		ok, shouldCompile, shouldMatch, pos := parseFowlerResult(fields[3])
		if !ok {
			return fmt.Errorf("%s:%d: cannot parse result %q", base, lineno, fields[3])
		}
		for _, c := range flag {
			pattern := fields[1]
			syn := syntax.POSIX | syntax.ClassNL
			switch c {
			default:
				continue
			case 'E':
			case 'L':
				pattern = regexp.QuoteMeta(pattern)
			}
			for _, c := range flag {
				if c == 'i' {
					syn |= syntax.FoldCase
				}
			}
			m := map[string]any{"file": base, "line": lineno, "mode": string(c), "flags": int(syn)}
			textOrHex(m, "pattern", pattern)
			textOrHex(m, "text", text)
			re, err := fowlerCompile(pattern, syn)
			if err != nil {
				var se *syntax.Error
				if !errors.As(err, &se) {
					return fmt.Errorf("%s:%d: %v", base, lineno, err)
				}
				if shouldCompile {
					return fmt.Errorf("%s:%d: %q did not compile", base, lineno, pattern)
				}
				if e := setError(m, err); e != nil {
					return e
				}
				lines.Add(m)
				continue
			}
			if !shouldCompile {
				return fmt.Errorf("%s:%d: %q should not compile", base, lineno, pattern)
			}
			match := re.MatchString(text)
			if match != shouldMatch {
				return fmt.Errorf("%s:%d: %q match %v, want %v", base, lineno, pattern, match, shouldMatch)
			}
			have := re.FindStringSubmatchIndex(text)
			if (len(have) > 0) != match {
				return fmt.Errorf("%s:%d: %q inconsistent match", base, lineno, pattern)
			}
			cut := have
			if len(cut) > len(pos) {
				cut = cut[:len(pos)]
			}
			if !slices.Equal(cut, pos) {
				return fmt.Errorf("%s:%d: %q have %v want %v", base, lineno, pattern, have, pos)
			}
			m["match"] = match
			m["have"] = ints(have)
			lines.Add(m)
		}
	}
	return nil
}

func fowlerCompile(pattern string, syn syntax.Flags) (*regexp.Regexp, error) {
	parsed, err := syntax.Parse(pattern, syn)
	if err != nil {
		return nil, err
	}
	s := parsed.String()
	again, err := syntax.Parse(s, syntax.Perl)
	if err != nil {
		return nil, fmt.Errorf("reparse %q: %v", s, err)
	}
	if !sameTree(parsed, again) {
		return nil, fmt.Errorf("reparse of %q as %q changes the tree", pattern, s)
	}
	re, err := regexp.Compile(s)
	if err != nil {
		return nil, fmt.Errorf("compile %q: %v", s, err)
	}
	re.Longest()
	return re, nil
}

func sameTree(a, b *syntax.Regexp) bool {
	if a.Op != b.Op || !slices.Equal(a.Rune, b.Rune) || a.Min != b.Min || a.Max != b.Max || a.Cap != b.Cap || a.Name != b.Name || len(a.Sub) != len(b.Sub) {
		return false
	}
	switch a.Op {
	case syntax.OpLiteral, syntax.OpCharClass:
		if a.Flags&syntax.FoldCase != b.Flags&syntax.FoldCase {
			return false
		}
	case syntax.OpStar, syntax.OpPlus, syntax.OpQuest, syntax.OpRepeat:
		if a.Flags&syntax.NonGreedy != b.Flags&syntax.NonGreedy {
			return false
		}
	}
	for i := range a.Sub {
		if !sameTree(a.Sub[i], b.Sub[i]) {
			return false
		}
	}
	return true
}

func parseFowlerResult(s string) (ok, compiled, matched bool, pos []int) {
	switch {
	case s == "":
		return true, true, true, nil
	case s == "NOMATCH":
		return true, true, false, nil
	case 'A' <= s[0] && s[0] <= 'Z':
		return true, false, false, nil
	}
	compiled = true
	var x []int
	for s != "" {
		var end byte = ')'
		if len(x)%2 == 0 {
			if s[0] != '(' {
				return false, compiled, false, nil
			}
			s = s[1:]
			end = ','
		}
		i := 0
		for i < len(s) && s[i] != end {
			i++
		}
		if i == 0 || i == len(s) {
			return false, compiled, false, nil
		}
		v := -1
		if s[:i] != "?" {
			n, err := strconv.Atoi(s[:i])
			if err != nil {
				return false, compiled, false, nil
			}
			v = n
		}
		x = append(x, v)
		s = s[i+1:]
	}
	if len(x)%2 != 0 {
		return false, compiled, false, nil
	}
	return true, compiled, true, x
}

type stringReader struct {
	s string
	i int
}

func (r *stringReader) ReadRune() (rune, int, error) {
	if r.i >= len(r.s) {
		return 0, 0, io.EOF
	}
	if c := r.s[r.i]; c < utf8.RuneSelf {
		r.i++
		return rune(c), 1, nil
	}
	c, size := utf8.DecodeRuneInString(r.s[r.i:])
	r.i += size
	return c, size, nil
}

var templates = []string{"<$0>", "[$1|${2}]", "$1x${1}x", "$$$x${name}${", "${0}$00$01$10"}

func pieces(m map[string]any, key string, parts []string) {
	if parts == nil {
		m[key] = nil
		return
	}
	valid := true
	for _, p := range parts {
		if !utf8.ValidString(p) {
			valid = false
		}
	}
	if valid {
		m[key] = parts
		return
	}
	hs := make([]string, len(parts))
	for i, p := range parts {
		hs[i] = hex.EncodeToString([]byte(p))
	}
	m[key+"_hex"] = hs
}

func (g *gen) execRecord(lines *goldenio.Lines, pattern string, posix bool, text string, repeat int) error {
	if err := g.compileRecord(pattern, posix); err != nil {
		return err
	}
	var re *regexp.Regexp
	var err error
	if posix {
		re, err = regexp.CompilePOSIX(pattern)
	} else {
		re, err = regexp.Compile(pattern)
	}
	if err != nil {
		return nil
	}
	m := map[string]any{"posix": posix}
	textOrHex(m, "pattern", pattern)
	textOrHex(m, "text", text)
	full := text
	if repeat > 1 {
		m["repeat"] = repeat
		full = strings.Repeat(text, repeat)
	}
	long := re.Copy()
	long.Longest()
	m["submatch"] = ints(re.FindStringSubmatchIndex(full))
	m["reader"] = ints(re.FindReaderSubmatchIndex(&stringReader{s: full}))
	m["longest"] = ints(long.FindStringSubmatchIndex(full))
	m["longest_reader"] = ints(long.FindReaderSubmatchIndex(&stringReader{s: full}))
	m["match"] = re.MatchString(full)
	m["match_reader"] = re.MatchReader(&stringReader{s: full})
	m["all_index_2"] = intss(re.FindAllIndex([]byte(full), 2))
	if repeat <= 1 {
		m["all"] = intss(re.FindAllStringSubmatchIndex(full, -1))
		m["all_longest"] = intss(long.FindAllStringSubmatchIndex(full, -1))
		pieces(m, "split", re.Split(full, -1))
		pieces(m, "split_2", re.Split(full, 2))
		var reps []string
		for _, t := range templates {
			reps = append(reps, re.ReplaceAllString(full, t))
		}
		reps = append(reps, re.ReplaceAllLiteralString(full, "<$1>"))
		reps = append(reps, re.ReplaceAllStringFunc(full, func(s string) string { return "<" + s + ">" }))
		pieces(m, "replace", reps)
	}
	lines.Add(m)
	return nil
}

var corpusTexts = []string{
	"",
	"a",
	"ab",
	"abc",
	"aaa",
	"aab",
	"abab",
	"aaabbb",
	"abcabc",
	"xyz",
	"hello world",
	"Hello World",
	"foo bar  baz",
	"a,b,,c,",
	"x\ny\n",
	"\n",
	"line1\nline2\r\nline3",
	"日本語",
	"日本語abc日本",
	"αβγ ΑΒΓ σς Σ",
	"Straße STRASSE strasse",
	"k K \u212a",
	"ſ s S",
	"Ωω\u2126",
	"ǅǄǆ",
	"\xff",
	"a\xffb",
	"\xc2\x80\xc2",
	"\xed\xa0\x80",
	"\xf4\x90\x80\x80x",
	"\xe6\x97",
	"2026-01-15 and 1999-12-31",
	"a1b2c3",
	"_word_ boundary-test",
	"\U0001F600x\U0001F600",
	"\x00\x01",
	"{a}[b](c)",
	"$1 ${name}",
	"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab",
}

var corpusPatterns = []string{
	``,
	`a`,
	`a(b)c`,
	`(a)(b)?(c)*`,
	`(a|ab)(c|bcd)(d*)`,
	`(a*)+`,
	`(a*)*`,
	`(a|b)*?c`,
	`x*`,
	`(?:a|b)+?`,
	`(a+)(b+)?`,
	`((a)|b)+`,
	`(?:(a)|b)+`,
	`(?P<first>\w+)\s+(?P<last>\w+)`,
	`(?<y>\d{4})-(?<m>\d{2})-(?<d>\d{2})`,
	`(?P<name>a)|(?P<name>b)`,
	`^`,
	`$`,
	`(?m)^a$`,
	`(?m)^`,
	`(?m)$`,
	`(?m)^line\d$`,
	`\A`,
	`\z`,
	`\Aab`,
	`ab\z`,
	`\b\w+\b`,
	`\b`,
	`\B`,
	`^$`,
	`(?m)^$`,
	`\p{Greek}+`,
	`\pL+`,
	`\p{Lu}`,
	`\PL+`,
	`[\p{Han}\p{Hiragana}]+`,
	`\p{Nd}+`,
	`[^\p{L}\p{N}]+`,
	`\p{^Greek}`,
	`\P{^Greek}+`,
	`\p{Any}`,
	`.`,
	`(?s).`,
	`.+`,
	`(?s).+`,
	`[^a]`,
	`[^a]+`,
	`日本語`,
	`[日本]+`,
	`[α-ω]+`,
	`\x{1F600}`,
	`[\x{10000}-\x{10FFFF}]`,
	`(?i)k`,
	`(?i)K`,
	`(?i)\x{212A}`,
	`(?i)s`,
	`(?i)ſ`,
	`(?i)σ`,
	`(?i)ς`,
	`(?i)Σ+`,
	`(?i)[a-z]+`,
	`(?i)straße`,
	`(?i)strasse`,
	`(?i)ǅ`,
	`(?i)[k-m]`,
	`(?i)Ω`,
	`(?i)ω`,
	`(?i)\x{2126}`,
	`(?i)[^k]`,
	`(?i)\p{Lu}`,
	`(?i)ΣΑΣ`,
	`(?i)[\x{100}-\x{17f}]+`,
	`(?i:a)b`,
	`(?i)a(?-i)b`,
	`[Kk]`,
	`[\x{212A}k]`,
	`\x{fffd}`,
	`[\x{fffd}]`,
	`a.b`,
	`[\x{80}-\x{10FFFF}]+`,
	`[\x00-\x{7f}]+`,
	`\x{D800}`,
	`[\x{D800}-\x{DFFF}]`,
	`[\x{10FFFF}]`,
	`a{1000}`,
	`a{0,1000}`,
	`(a{2}){500}`,
	`((a{10}){10}){10}`,
	`a{2,5}`,
	`a{2,}`,
	`a{,5}`,
	`x{1001`,
	`(?:a{3}){0}b`,
	`(a){0}b`,
	`\pL{3,5}`,
	`.{3}`,
	`^(a+)(b+)$`,
	`^(?:(a)|b)+$`,
	`^a(/b+(#c+)*)*$`,
	`^.bc(d|e)*$`,
	`^[a-c]*`,
	`(x+x+)+y`,
	`(a|aa)*b`,
	`a(|b)`,
	`(a|ab)(c|bcd)`,
	`a*?`,
	`(a+?)(b*)`,
	`.*?x`,
	`(?U)a+`,
	`(?U)a+?`,
	`a??`,
	`|a`,
	`(?:)`,
	`,`,
	`a*`,
	`\s+`,
	`\d+`,
	`\w+`,
	`\W+`,
	`\S+`,
	`\D`,
	`[[:alpha:]]+`,
	`[[:^alpha:]]+`,
	`[[:upper:][:digit:]]+`,
	`[\d\-z]+`,
	`\pN+`,
	`\x41\x{42}\103`,
	`\0`,
	`\_`,
	`\Q$1.(\E+`,
	`(?s:.)(?-s:.)`,
	`(?i-s:A.)`,
	`[^\n]+`,
	`[a\-\]z]+`,
	`\{\}\[\]\(\)`,
	`(?:a|b|c|d|e|f|g|h)(?:x|y)`,
	`abc|abd|aef|bcx|bcy`,
	`(?:hello|help|helm)+`,
	`one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve`,
	`(a)|b|(c)|d`,
	`((((((((((a))))))))))`,
	`(a(b(c(d(e)))))`,
	`(?:(?:(?:a*)*)*)*b`,
	`(a*)(a*)(a*)`,
	`(?:x|xy)(?:z|yz)`,
	`[a-c]+?b`,
	`(\$)(\d)`,
	`\$\{name\}`,
	`\x00`,
	`[^\x00-\x{10FFFF}]`,
	`[\s\S]`,
	`(?i)[^\s\S]`,
}

var corpusErrors = []string{
	`*`,
	`+`,
	`?`,
	`(abc`,
	`abc)`,
	`x[a-z`,
	`[z-a]`,
	`abc\`,
	`a**`,
	`a*+`,
	`a??+`,
	`\x`,
	`\8`,
	`\C`,
	`\x{110000}`,
	`\p{Foo}`,
	`\pX`,
	`[[:foo:]]`,
	`(?P<x y>a)`,
	`(?P<>a)`,
	`(?<name`,
	`(?i`,
	`(?z)`,
	`(?-)`,
	`(?i-)`,
	`a{1001}`,
	`a{2,1}`,
	`(a{2}){501}`,
	`((a{10}){10}){11}`,
	`a{100000}`,
	"\xff",
	"a\xffb",
	"[\xff]",
	"\\\xff",
	"\\Q\xff\\E",
	"(?P<\xff>a)",
	"\\p{\xff}",
	`[a-\d]`,
	`[\d-z]`,
	`\Q\E*`,
	`(?:a{1000}){1000}`,
	`x{2}{2}`,
	`a|*`,
	`()+*`,
	`(?P=name)`,
	`(?#comment)`,
	`[a`,
	`[]a`,
	`[^]`,
	`\pN{2}{3}`,
}

func (g *gen) corpus() error {
	var lines goldenio.Lines
	for _, p := range corpusPatterns {
		for _, t := range corpusTexts {
			if err := g.execRecord(&lines, p, false, t, 1); err != nil {
				return err
			}
		}
	}
	posix := []string{`a+|ab`, `(a|ab)(c|bcd)(d*)`, `[[:alpha:]]+`, `(a*)+`, `(a|b)*c`, `x*`, `^a|b$`, `[^a]`, `.`, `(.*)(.*)`, `a\b`, `(?i)a`, `\pL`, `[[:digit:]]+|[[:alpha:]]+`}
	for _, p := range posix {
		for _, t := range corpusTexts {
			if err := g.execRecord(&lines, p, true, t, 1); err != nil {
				return err
			}
		}
	}
	for _, p := range corpusErrors {
		if err := g.compileRecord(p, false); err != nil {
			return err
		}
		if err := g.compileRecord(p, true); err != nil {
			return err
		}
	}
	big := []string{
		g.build(part{"(", 999}, part{"a", 1}, part{")", 999}),
		g.build(part{"(", 1000}, part{"a", 1}, part{")", 1000}),
		g.build(part{"(?:", 999}, part{"a", 1}, part{")*", 999}),
		g.build(part{"(?:", 1000}, part{"a", 1}, part{")*", 1000}),
		g.build(part{`\pL`, 27000}),
		g.build(part{`\pL`, 25458}),
		g.build(part{`\pL`, 25459}),
		g.build(part{"(xx?){1000}", 1000}),
		g.build(part{"(", 1}, part{"(xx?)", 1000}, part{"){1000}", 1}),
		g.build(part{"(?:x{1000})", 3355}),
		g.build(part{"(?:x{1000})", 3356}),
		g.build(part{"(", 1}, part{"|", 12345}, part{")", 1}),
		g.build(part{"a|", 3000}, part{"b", 1}),
		g.build(part{`\pL{1000}`, 4}),
		g.build(part{"((((((((((x{2}){2}){2}){2}){2}){2}){2}){2}){2}){2})", 1}),
		g.build(part{"((((((((((x{2}){2}){2}){2}){2}){2}){2}){2}){2}))", 1}),
		g.build(part{"(?:a{1000})", 3}, part{"(?:b{2}){500}", 2}),
		g.build(part{"x{1000}", 1}, part{"(?:", 998}, part{"y", 1}, part{")", 998}),
	}
	for _, p := range big {
		if err := g.compileRecord(p, false); err != nil {
			return err
		}
	}
	longCases := []struct {
		pattern string
		text    string
		repeat  int
	}{
		{`(a+)(b+)`, "ab", 3000},
		{`(a+)(b+)`, "aab", 2000},
		{`(x+x+)+y`, "x", 5000},
		{`(x+x+)+y`, "x", 40},
		{`^(a+)(b+)$`, "ab", 3000},
		{`b(c)`, "aaaa", 3000},
		{`(?i)Z\b`, "zzzzz ", 2000},
		{`(a|ab)(c|bcd)(d*)`, "abcd", 3000},
		{`\p{Greek}+(x)`, "αβγ", 3000},
		{`.*`, "日本\xff", 3000},
		{`[a-c]+$`, "abcabcd", 2000},
		{`(?m)^b$`, "a\nb\n", 3000},
		{`y`, "x", 100000},
		{`(\w+)@(\w+)\.com`, "spam ", 4000},
		{`a{100}`, "a", 300},
		{`(?:a|b)*c`, "ab", 70000},
	}
	for _, lc := range longCases {
		if err := g.execRecord(&lines, lc.pattern, false, lc.text, lc.repeat); err != nil {
			return err
		}
	}
	return lines.Write(g.out, "exec.jsonl")
}

func dump(re *syntax.Regexp) string {
	var b strings.Builder
	dumpRegexp(&b, re)
	return b.String()
}

var opNames = []string{
	syntax.OpNoMatch:        "no",
	syntax.OpEmptyMatch:     "emp",
	syntax.OpLiteral:        "lit",
	syntax.OpCharClass:      "cc",
	syntax.OpAnyCharNotNL:   "dnl",
	syntax.OpAnyChar:        "dot",
	syntax.OpBeginLine:      "bol",
	syntax.OpEndLine:        "eol",
	syntax.OpBeginText:      "bot",
	syntax.OpEndText:        "eot",
	syntax.OpWordBoundary:   "wb",
	syntax.OpNoWordBoundary: "nwb",
	syntax.OpCapture:        "cap",
	syntax.OpStar:           "star",
	syntax.OpPlus:           "plus",
	syntax.OpQuest:          "que",
	syntax.OpRepeat:         "rep",
	syntax.OpConcat:         "cat",
	syntax.OpAlternate:      "alt",
}

func dumpRegexp(b *strings.Builder, re *syntax.Regexp) {
	if int(re.Op) >= len(opNames) || opNames[re.Op] == "" {
		fmt.Fprintf(b, "op%d", re.Op)
	} else {
		switch re.Op {
		default:
			b.WriteString(opNames[re.Op])
		case syntax.OpStar, syntax.OpPlus, syntax.OpQuest, syntax.OpRepeat:
			if re.Flags&syntax.NonGreedy != 0 {
				b.WriteByte('n')
			}
			b.WriteString(opNames[re.Op])
		case syntax.OpLiteral:
			if len(re.Rune) > 1 {
				b.WriteString("str")
			} else {
				b.WriteString("lit")
			}
			if re.Flags&syntax.FoldCase != 0 {
				for _, r := range re.Rune {
					if unicode.SimpleFold(r) != r {
						b.WriteString("fold")
						break
					}
				}
			}
		}
	}
	b.WriteByte('{')
	switch re.Op {
	case syntax.OpEndText:
		if re.Flags&syntax.WasDollar == 0 {
			b.WriteString(`\z`)
		}
	case syntax.OpLiteral:
		for _, r := range re.Rune {
			b.WriteRune(r)
		}
	case syntax.OpConcat, syntax.OpAlternate:
		for _, sub := range re.Sub {
			dumpRegexp(b, sub)
		}
	case syntax.OpStar, syntax.OpPlus, syntax.OpQuest:
		dumpRegexp(b, re.Sub[0])
	case syntax.OpRepeat:
		fmt.Fprintf(b, "%d,%d ", re.Min, re.Max)
		dumpRegexp(b, re.Sub[0])
	case syntax.OpCapture:
		if re.Name != "" {
			b.WriteString(re.Name)
			b.WriteByte(':')
		}
		dumpRegexp(b, re.Sub[0])
	case syntax.OpCharClass:
		sep := ""
		for i := 0; i < len(re.Rune); i += 2 {
			b.WriteString(sep)
			sep = " "
			lo, hi := re.Rune[i], re.Rune[i+1]
			if lo == hi {
				fmt.Fprintf(b, "%#x", lo)
			} else {
				fmt.Fprintf(b, "%#x-%#x", lo, hi)
			}
		}
	}
	b.WriteByte('}')
}

func (g *gen) syntaxRecord(lines *goldenio.Lines, pattern string, flags syntax.Flags) error {
	m := map[string]any{"flags": int(flags)}
	if err := g.patternField(m, pattern); err != nil {
		return err
	}
	re, err := syntax.Parse(pattern, flags)
	if err != nil {
		if e := setError(m, err); e != nil {
			return e
		}
		lines.Add(m)
		return nil
	}
	bigText(m, "dump", dump(re))
	bigText(m, "string", re.String())
	m["max_cap"] = re.MaxCap()
	m["cap_names"] = re.CapNames()
	simple := re.Simplify()
	bigText(m, "simplify", simple.String())
	bigText(m, "simplify_dump", dump(simple))
	prog, err := syntax.Compile(simple)
	if err != nil {
		return err
	}
	bigText(m, "prog", prog.String())
	prefix, complete := prog.Prefix()
	textOrHex(m, "prefix", prefix)
	m["complete"] = complete
	m["start_cond"] = int(prog.StartCond())
	lines.Add(m)
	return nil
}

func (g *gen) syntaxCorpus() error {
	var lines goldenio.Lines
	flagSets := []syntax.Flags{
		syntax.Perl,
		syntax.POSIX,
		syntax.Perl | syntax.FoldCase,
		syntax.MatchNL | syntax.PerlX | syntax.UnicodeGroups,
		syntax.Perl &^ syntax.OneLine,
		syntax.POSIX | syntax.ClassNL | syntax.FoldCase,
		syntax.Literal,
		syntax.Literal | syntax.FoldCase,
		syntax.Perl | syntax.NonGreedy | syntax.DotNL,
	}
	all := append(append([]string{}, corpusPatterns...), corpusErrors...)
	all = append(all, `(?i)[[:lower:]]`, `(?i)[^[:lower:]]`, `(?i)\W`, `[[:word:]]`, `x(?i:ab*c|d?e)1`, `0(?i:ab*c|d?e)1`, `A(?:[Bb][Cc]|[Dd])[Zz]`, `[Aa](?:[Bb][Cc]|[Dd])Z`, `(?:a{1,}){1,}`, `(a){0,4}`, `(a){2,6}`, `(){0,2}`, `a{0}`, `(?:x|(?:xa))`, `(?i)abc|ABD`, `x{2}y|x{2}[0-9]y`, `a.*?c|a.*?b`, `[[:cntrl:][:^cntrl:]]`, `[^[:cntrl:][:^cntrl:]]`, `(?i)[\x00-\x{FFFD}]`, `(?-m)$`, `(?m)\z`, `\Q\\E`, `[\Q]\E]`, `(?U)(?:a+?b*)`, `a+?b??c*?d{2,3}?`)
	for _, p := range all {
		for _, f := range flagSets {
			if err := g.syntaxRecord(&lines, p, f); err != nil {
				return err
			}
		}
	}
	var names []string
	for name := range unicode.Categories {
		names = append(names, name)
	}
	for name := range unicode.Scripts {
		names = append(names, name)
	}
	names = append(names, "Any")
	sort.Strings(names)
	for _, name := range names {
		for _, p := range []string{`\p{` + name + `}`, `\P{` + name + `}`, `(?i)\p{` + name + `}`, `(?i)[^\p{` + name + `}a]`} {
			if err := g.syntaxRecord(&lines, p, syntax.Perl); err != nil {
				return err
			}
		}
	}
	for _, p := range []string{`\pL`, `\pN`, `\pZ`, `\PC`, `\p{^L}`, `\pM`} {
		if err := g.syntaxRecord(&lines, p, syntax.Perl); err != nil {
			return err
		}
	}
	return lines.Write(g.out, "syntax.jsonl")
}

func (g *gen) fuzz() error {
	var lines goldenio.Lines
	rng := rand.New(rand.NewSource(20261124))
	atoms := []string{"a", "b", "c", "A", "B", ".", "[ab]", "[^a]", "[a-c]", `\w`, `\s`, `\d`, "^", "$", `\b`, `\B`, "(?:)", "日", "σ", "Σ", "ς", "k", "K", `\x{212a}`, `\pL`, `\p{Greek}`, "x", `\n`, "(?s:.)", `\A`, `\z`, "(?m:^)", "(?m:$)"}
	alphabet := []string{"a", "b", "c", "A", "B", "x", " ", "\n", "日", "σ", "Σ", "ς", "k", "K", "\u212a", "1", "_", "\xff", "é"}
	var gen func(depth int) string
	gen = func(depth int) string {
		if depth <= 0 {
			return atoms[rng.Intn(len(atoms))]
		}
		switch rng.Intn(10) {
		case 0, 1:
			return gen(depth-1) + gen(depth-1)
		case 2:
			return gen(depth-1) + "|" + gen(depth-1)
		case 3:
			return "(" + gen(depth-1) + ")"
		case 4:
			return "(?:" + gen(depth-1) + ")" + []string{"*", "+", "?", "*?", "+?", "??"}[rng.Intn(6)]
		case 5:
			return "(" + gen(depth-1) + ")" + []string{"{2}", "{1,3}", "{0,2}", "{2,}", "{0}", "{1}?"}[rng.Intn(6)]
		case 6:
			return "(?i:" + gen(depth-1) + ")"
		case 7:
			return gen(depth-1) + []string{"*", "+", "?"}[rng.Intn(3)]
		default:
			return atoms[rng.Intn(len(atoms))]
		}
	}
	for n := 0; n < 4000; n++ {
		pattern := gen(1 + rng.Intn(4))
		var texts []string
		for k := 0; k < 8; k++ {
			var b strings.Builder
			length := rng.Intn(9)
			for j := 0; j < length; j++ {
				b.WriteString(alphabet[rng.Intn(len(alphabet))])
			}
			texts = append(texts, b.String())
		}
		re, err := regexp.Compile(pattern)
		m := map[string]any{}
		textOrHex(m, "pattern", pattern)
		pieces(m, "texts", texts)
		summary, e := compileSummary(re, err)
		if e != nil {
			return e
		}
		var b strings.Builder
		b.WriteString(summary)
		b.WriteByte('\n')
		if err == nil {
			long := re.Copy()
			long.Longest()
			for _, t := range texts {
				b.WriteString(resText(re.FindStringSubmatchIndex(t)))
				b.WriteString(resText(re.FindReaderSubmatchIndex(&stringReader{s: t})))
				b.WriteString(resText(long.FindStringSubmatchIndex(t)))
				b.WriteString(resText(long.FindReaderSubmatchIndex(&stringReader{s: t})))
				b.WriteString(allText(re.FindAllStringSubmatchIndex(t, -1)))
				b.WriteString(allText(long.FindAllStringSubmatchIndex(t, -1)))
				b.WriteString(boolText(re.MatchString(t)))
				b.WriteByte('\n')
			}
		}
		m["fnv"] = fnv(b.String())
		lines.Add(m)
	}
	return lines.Write(g.out, "fuzz.jsonl")
}
