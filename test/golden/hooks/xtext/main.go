package main

import (
	"encoding/binary"
	"encoding/hex"
	"flag"
	"fmt"
	"go/ast"
	"go/constant"
	"go/parser"
	"go/token"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/taymour/elysiandb/test/golden/goldenio"
	"golang.org/x/text/cases"
	"golang.org/x/text/collate"
	"golang.org/x/text/language"
	"golang.org/x/text/unicode/norm"
)

type field struct {
	Name string `json:"name"`
	Type string `json:"type"`
}

type entry struct {
	Source string  `json:"source"`
	Name   string  `json:"name"`
	Kind   string  `json:"kind"`
	Elem   string  `json:"elem,omitempty"`
	Fields []field `json:"fields,omitempty"`
	Count  int     `json:"count,omitempty"`
	Bytes  int     `json:"bytes"`
	File   string  `json:"file,omitempty"`
	Value  string  `json:"value,omitempty"`
}

var widths = map[string]int{"uint8": 1, "byte": 1, "int8": 1, "uint16": 2, "int16": 2, "uint32": 4, "int32": 4, "rune": 4, "uint64": 8, "int64": 8, "int": 8, "uint": 8}

func main() {
	out := flag.String("out", "", "tests/golden/hooks")
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "missing -out")
		os.Exit(2)
	}
	if err := run(*out); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(out string) error {
	dirOut, err := exec.Command("go", "list", "-m", "-f", "{{.Dir}}", "golang.org/x/text").Output()
	if err != nil {
		return err
	}
	root := strings.TrimSpace(string(dirOut))
	sources := []struct{ pkg, file, dest string }{
		{"collate", "tables.go", "collate"},
		{"unicode/norm", "tables15.0.0.go", "norm"},
		{"cases", "tables15.0.0.go", "cases"},
	}
	var layout []entry
	for _, s := range sources {
		entries, err := extract(out, filepath.Join(root, s.pkg), s.file, s.dest, s.pkg)
		if err != nil {
			return fmt.Errorf("%s/%s: %w", s.pkg, s.file, err)
		}
		layout = append(layout, entries...)
	}
	if err := goldenio.WriteJSON(out, "xtext/layout.json", map[string]any{
		"module":  "golang.org/x/text v0.31.0",
		"endian":  "little",
		"note":    "each array element is written field by field in declaration order with the listed widths; strings are their raw bytes",
		"entries": layout,
	}); err != nil {
		return err
	}
	return vectors(out)
}

func structTypes(pkgDir string) (map[string][]field, error) {
	fset := token.NewFileSet()
	pkgs, err := parser.ParseDir(fset, pkgDir, func(fi os.FileInfo) bool { return !strings.HasSuffix(fi.Name(), "_test.go") }, 0)
	if err != nil {
		return nil, err
	}
	types := map[string][]field{}
	for _, p := range pkgs {
		for _, f := range p.Files {
			for _, d := range f.Decls {
				gd, ok := d.(*ast.GenDecl)
				if !ok || gd.Tok != token.TYPE {
					continue
				}
				for _, sp := range gd.Specs {
					ts := sp.(*ast.TypeSpec)
					if st, ok := ts.Type.(*ast.StructType); ok {
						types[ts.Name.Name] = structFields(st)
					}
				}
			}
		}
	}
	return types, nil
}

func structFields(st *ast.StructType) []field {
	var fs []field
	for _, f := range st.Fields.List {
		tname := fmt.Sprint(f.Type)
		for _, n := range f.Names {
			fs = append(fs, field{Name: n.Name, Type: tname})
		}
	}
	return fs
}

func evalInt(e ast.Expr, consts map[string]constant.Value) (int64, bool) {
	switch x := e.(type) {
	case *ast.BasicLit:
		v := constant.MakeFromLiteral(x.Value, x.Kind, 0)
		if v.Kind() == constant.Int {
			n, ok := constant.Int64Val(v)
			return n, ok
		}
	case *ast.UnaryExpr:
		if x.Op == token.SUB {
			n, ok := evalInt(x.X, consts)
			return -n, ok
		}
	case *ast.Ident:
		if v, ok := consts[x.Name]; ok && v.Kind() == constant.Int {
			n, ok := constant.Int64Val(v)
			return n, ok
		}
	case *ast.ParenExpr:
		return evalInt(x.X, consts)
	}
	return 0, false
}

func evalString(e ast.Expr) (string, bool) {
	switch x := e.(type) {
	case *ast.BasicLit:
		if x.Kind == token.STRING {
			s, err := strconv.Unquote(x.Value)
			return s, err == nil
		}
	case *ast.BinaryExpr:
		if x.Op == token.ADD {
			a, ok1 := evalString(x.X)
			b, ok2 := evalString(x.Y)
			return a + b, ok1 && ok2
		}
	case *ast.ParenExpr:
		return evalString(x.X)
	}
	return "", false
}

func putInt(buf []byte, width int, v int64) []byte {
	var tmp [8]byte
	binary.LittleEndian.PutUint64(tmp[:], uint64(v))
	return append(buf, tmp[:width]...)
}

func extract(out, pkgDir, file, dest, pkg string) ([]entry, error) {
	types, err := structTypes(pkgDir)
	if err != nil {
		return nil, err
	}
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, filepath.Join(pkgDir, file), nil, 0)
	if err != nil {
		return nil, err
	}
	consts := map[string]constant.Value{}
	var entries []entry
	for _, d := range f.Decls {
		gd, ok := d.(*ast.GenDecl)
		if !ok || (gd.Tok != token.VAR && gd.Tok != token.CONST) {
			continue
		}
		for _, sp := range gd.Specs {
			vs := sp.(*ast.ValueSpec)
			for i, name := range vs.Names {
				if i >= len(vs.Values) {
					continue
				}
				val := vs.Values[i]
				if s, ok := evalString(val); ok {
					consts[name.Name] = constant.MakeString(s)
					rel := "xtext/" + dest + "/" + name.Name + ".bin"
					if err := goldenio.WriteFile(out, rel, []byte(s)); err != nil {
						return nil, err
					}
					entries = append(entries, entry{Source: pkg + "/" + file, Name: name.Name, Kind: "string", Bytes: len(s), File: rel})
					continue
				}
				if n, ok := evalInt(val, consts); ok {
					consts[name.Name] = constant.MakeInt64(n)
					entries = append(entries, entry{Source: pkg + "/" + file, Name: name.Name, Kind: "int", Value: strconv.FormatInt(n, 10)})
					continue
				}
				cl, ok := val.(*ast.CompositeLit)
				if !ok {
					continue
				}
				var eltType ast.Expr
				switch t := cl.Type.(type) {
				case *ast.ArrayType:
					eltType = t.Elt
				default:
					continue
				}
				var fields []field
				elem := ""
				switch et := eltType.(type) {
				case *ast.Ident:
					if _, basic := widths[et.Name]; basic {
						elem = et.Name
					} else if fs, ok := types[et.Name]; ok {
						fields = fs
						elem = et.Name
					}
				case *ast.StructType:
					fields = structFields(et)
					elem = "struct"
				}
				if elem == "" {
					continue
				}
				values := map[int][]int64{}
				maxIndex := -1
				next := 0
				for _, el := range cl.Elts {
					idx := next
					v := el
					if kv, ok := el.(*ast.KeyValueExpr); ok {
						k, ok := evalInt(kv.Key, consts)
						if !ok {
							return nil, fmt.Errorf("%s: non-integer key", name.Name)
						}
						idx = int(k)
						v = kv.Value
					}
					var nums []int64
					if fields == nil {
						n, ok := evalInt(v, consts)
						if !ok {
							return nil, fmt.Errorf("%s[%d]: not an integer", name.Name, idx)
						}
						nums = []int64{n}
					} else {
						sub, ok := v.(*ast.CompositeLit)
						if !ok {
							return nil, fmt.Errorf("%s[%d]: not a struct literal", name.Name, idx)
						}
						nums = make([]int64, len(fields))
						for fi, se := range sub.Elts {
							pos := fi
							sv := se
							if kv, ok := se.(*ast.KeyValueExpr); ok {
								key := kv.Key.(*ast.Ident).Name
								for j, fd := range fields {
									if fd.Name == key {
										pos = j
									}
								}
								sv = kv.Value
							}
							n, ok := evalInt(sv, consts)
							if !ok {
								return nil, fmt.Errorf("%s[%d]: field not an integer", name.Name, idx)
							}
							nums[pos] = n
						}
					}
					values[idx] = nums
					if idx > maxIndex {
						maxIndex = idx
					}
					next = idx + 1
				}
				count := maxIndex + 1
				if at, ok := cl.Type.(*ast.ArrayType); ok && at.Len != nil {
					if n, ok := evalInt(at.Len, consts); ok {
						count = int(n)
					}
				}
				var buf []byte
				for i := 0; i < count; i++ {
					nums := values[i]
					if fields == nil {
						v := int64(0)
						if nums != nil {
							v = nums[0]
						}
						buf = putInt(buf, widths[elem], v)
						continue
					}
					for j, fd := range fields {
						v := int64(0)
						if nums != nil {
							v = nums[j]
						}
						buf = putInt(buf, widths[fd.Type], v)
					}
				}
				rel := "xtext/" + dest + "/" + name.Name + ".bin"
				if err := goldenio.WriteFile(out, rel, buf); err != nil {
					return nil, err
				}
				e := entry{Source: pkg + "/" + file, Name: name.Name, Kind: "array", Elem: elem, Fields: fields, Count: count, Bytes: len(buf), File: rel}
				entries = append(entries, e)
			}
		}
	}
	sort.SliceStable(entries, func(i, j int) bool { return entries[i].Name < entries[j].Name })
	return entries, nil
}

func vectors(out string) error {
	var nl goldenio.Lines
	forms := []struct {
		name string
		f    norm.Form
	}{{"NFC", norm.NFC}, {"NFD", norm.NFD}, {"NFKC", norm.NFKC}, {"NFKD", norm.NFKD}}
	for c := rune(0); c <= 0x10ffff; c++ {
		if c >= 0xd800 && c <= 0xdfff {
			continue
		}
		s := string(c)
		row := map[string]any{}
		for _, fm := range forms {
			if got := fm.f.String(s); got != s {
				row[fm.name] = hex.EncodeToString([]byte(got))
			}
		}
		if p := norm.NFC.PropertiesString(s); p.CCC() != 0 {
			row["ccc"] = p.CCC()
		}
		if len(row) > 0 {
			row["cp"] = c
			nl.Add(row)
		}
	}
	samples := []string{"e\u0301", "\u1100\u1161\u11a8", "\uac00\u11a8", "a\u0323\u0302", "a\u0302\u0323", "\u212b", "\ufb01", "\u00c5\u0301", "a" + strings.Repeat("\u0301", 35), "\u0958", "\u2126\u0301", "\xff\u0301", "x\u0345\u0301"}
	for _, s := range samples {
		row := map[string]any{"in": hex.EncodeToString([]byte(s))}
		for _, fm := range forms {
			row[fm.name] = hex.EncodeToString([]byte(fm.f.String(s)))
		}
		nl.Add(row)
	}
	if err := nl.Write(out, "xtext/norm/vectors.jsonl"); err != nil {
		return err
	}
	var cl goldenio.Lines
	col := collate.New(language.Und)
	var words []string
	for c := rune(0x20); c < 0x2500; c += 7 {
		if utf8.ValidRune(c) {
			words = append(words, string(c))
		}
	}
	words = append(words, "a", "A", "\u00e1", "b", "ab", "aB", "Ab", "\u00e6", "ae", "\u0153", "ss", "\u00df", "\u00e9t\u00e9", "ete", "Ete", "\u4e2d", "\u0430", "\u0627", "\u05d0", "1", "10", "2", "-", " ", "", "a\u0300", "\u00e0", "\u1100\u1161", "\uac00", "z", "\U0001f600")
	type keyed struct{ s, key string }
	var list []keyed
	var buf collate.Buffer
	for _, w := range words {
		nfd := norm.NFD.String(w)
		list = append(list, keyed{w, hex.EncodeToString(col.KeyFromString(&buf, nfd))})
		buf.Reset()
	}
	for _, k := range list {
		cl.Add(map[string]any{"in": hex.EncodeToString([]byte(k.s)), "key_nfd": k.key})
	}
	for i := 0; i+1 < len(words); i++ {
		a, b := norm.NFD.String(words[i]), norm.NFD.String(words[i+1])
		cl.Add(map[string]any{"a": hex.EncodeToString([]byte(words[i])), "b": hex.EncodeToString([]byte(words[i+1])), "compare_nfd": col.CompareString(a, b)})
	}
	if err := cl.Write(out, "xtext/collate/vectors.jsonl"); err != nil {
		return err
	}
	var cs goldenio.Lines
	upper := cases.Upper(language.Und)
	lower := cases.Lower(language.Und)
	for c := rune(0); c <= 0x10ffff; c++ {
		if c >= 0xd800 && c <= 0xdfff {
			continue
		}
		s := string(c)
		u, l := upper.String(s), lower.String(s)
		if u != s || l != s {
			cs.Add(map[string]any{"cp": c, "upper": hex.EncodeToString([]byte(u)), "lower": hex.EncodeToString([]byte(l))})
		}
	}
	for _, s := range []string{"\u0391\u03a3", "\u0391\u03a3 \u0391", "\u03a3", "\u00df", "\u0130", "i\u0307", "I\u0307", "\ufb03", "\u01c5", "\u0345", "a\u0345\u0301", "\xffA"} {
		cs.Add(map[string]any{"in": hex.EncodeToString([]byte(s)), "upper": hex.EncodeToString([]byte(upper.String(s))), "lower": hex.EncodeToString([]byte(lower.String(s)))})
	}
	return cs.Write(out, "xtext/cases/vectors.jsonl")
}
