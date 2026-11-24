package main

import (
	"bufio"
	"bytes"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"math/big"
	"os"
	"reflect"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/dop251/goja/ast"
	"github.com/dop251/goja/file"
	"github.com/dop251/goja/parser"
	"github.com/dop251/goja/token"
	"github.com/dop251/goja/unistring"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

var (
	fileType      = reflect.TypeOf((*file.File)(nil))
	bigIntType    = reflect.TypeOf((*big.Int)(nil))
	unistringType = reflect.TypeOf(unistring.String(""))
	tokenType     = reflect.TypeOf(token.Token(0))
	idxType       = reflect.TypeOf(file.Idx(0))
)

type dumper struct {
	b    strings.Builder
	ids  map[uintptr]int
	next int
}

func (d *dumper) value(v reflect.Value) {
	switch v.Kind() {
	case reflect.Invalid:
		d.b.WriteString("nil")
	case reflect.Interface:
		if v.IsNil() {
			d.b.WriteString("nil")
			return
		}
		d.value(v.Elem())
	case reflect.Pointer:
		if v.IsNil() {
			d.b.WriteString("nil")
			return
		}
		switch v.Type() {
		case fileType:
			d.b.WriteString("<file>")
			return
		case bigIntType:
			d.b.WriteString("bigint(" + v.Interface().(*big.Int).String() + ")")
			return
		}
		p := v.Pointer()
		if id, ok := d.ids[p]; ok {
			fmt.Fprintf(&d.b, "*#%d", id)
			return
		}
		d.next++
		d.ids[p] = d.next
		fmt.Fprintf(&d.b, "*#%d ", d.next)
		d.value(v.Elem())
	case reflect.Struct:
		t := v.Type()
		d.b.WriteString(t.Name())
		d.b.WriteByte('{')
		first := true
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			if !f.IsExported() {
				continue
			}
			if !first {
				d.b.WriteByte(' ')
			}
			first = false
			d.b.WriteString(f.Name)
			d.b.WriteByte(':')
			d.value(v.Field(i))
		}
		d.b.WriteByte('}')
	case reflect.Slice:
		d.b.WriteByte('[')
		for i := 0; i < v.Len(); i++ {
			if i > 0 {
				d.b.WriteByte(' ')
			}
			d.value(v.Index(i))
		}
		d.b.WriteByte(']')
	case reflect.String:
		if v.Type() == unistringType {
			s := v.Interface().(unistring.String)
			if s.AsUtf16() != nil {
				d.b.WriteString("u" + strconv.Quote(s.String()))
				return
			}
		}
		d.b.WriteString(strconv.Quote(v.String()))
	case reflect.Int:
		switch v.Type() {
		case tokenType:
			fmt.Fprintf(&d.b, "T%d", v.Int())
		case idxType:
			fmt.Fprintf(&d.b, "%d", v.Int())
		default:
			fmt.Fprintf(&d.b, "int(%d)", v.Int())
		}
	case reflect.Int64:
		fmt.Fprintf(&d.b, "int64(%d)", v.Int())
	case reflect.Float64:
		fmt.Fprintf(&d.b, "float64(%016x)", math.Float64bits(v.Float()))
	case reflect.Bool:
		if v.Bool() {
			d.b.WriteString("true")
		} else {
			d.b.WriteString("false")
		}
	default:
		panic("unexpected kind " + v.Kind().String())
	}
}

func dumpAST(prg *ast.Program) string {
	d := &dumper{ids: map[uintptr]int{}}
	d.value(reflect.ValueOf(prg))
	return d.b.String()
}

func walkNodes(v reflect.Value, fn func(any) bool) {
	switch v.Kind() {
	case reflect.Interface, reflect.Pointer:
		if v.IsNil() {
			return
		}
		if v.Kind() == reflect.Pointer && !fn(v.Interface()) {
			return
		}
		walkNodes(v.Elem(), fn)
	case reflect.Struct:
		for i := 0; i < v.NumField(); i++ {
			if v.Type().Field(i).IsExported() {
				walkNodes(v.Field(i), fn)
			}
		}
	case reflect.Slice:
		for i := 0; i < v.Len(); i++ {
			walkNodes(v.Index(i), fn)
		}
	}
}

func walkTypes(prg *ast.Program) string {
	var types []string
	walkNodes(reflect.ValueOf(prg), func(n any) bool {
		types = append(types, strings.TrimPrefix(fmt.Sprintf("%T", n), "*ast."))
		return true
	})
	return strings.Join(types, " ")
}

func walkPruned(prg *ast.Program) string {
	var types []string
	walkNodes(reflect.ValueOf(prg), func(n any) bool {
		types = append(types, strings.TrimPrefix(fmt.Sprintf("%T", n), "*ast."))
		switch n.(type) {
		case *ast.FunctionLiteral, *ast.PropertyKeyed, *ast.CallExpression:
			return false
		}
		return true
	})
	return strings.Join(types, " ")
}

func astRecord(label, script string) map[string]any {
	m := map[string]any{"label": label}
	if utf8.ValidString(script) {
		m["script"] = script
	} else {
		m["script_hex"] = hex.EncodeToString([]byte(script))
	}
	prg, err := parser.ParseFile(nil, "", script, 0, parser.WithDisableSourceMaps)
	if err != nil {
		m["error"] = err.Error()
	} else {
		m["error"] = ""
	}
	m["dump"] = dumpAST(prg)
	m["walk"] = walkTypes(prg)
	m["walk_pruned"] = walkPruned(prg)
	_, errDefault := parser.ParseFile(nil, "", script, 0)
	m["default_fails"] = errDefault != nil
	return m
}

func genAST(out string) error {
	data, err := os.ReadFile("tests/golden/hooks/corpus-utc.jsonl")
	if err != nil {
		return err
	}
	var lines goldenio.Lines
	sc := bufio.NewScanner(bytes.NewReader(data))
	sc.Buffer(make([]byte, 1<<20), 1<<24)
	for sc.Scan() {
		var row struct {
			Label  string `json:"label"`
			Script string `json:"script"`
		}
		if err := json.Unmarshal(sc.Bytes(), &row); err != nil {
			return err
		}
		lines.Add(astRecord(row.Label, row.Script))
	}
	if err := sc.Err(); err != nil {
		return err
	}
	if err := lines.Write(out, "ast/corpus.jsonl"); err != nil {
		return err
	}
	var extra goldenio.Lines
	for i, s := range extraScripts {
		extra.Add(astRecord("extra-"+strconv.Itoa(i), s))
	}
	return extra.Write(out, "ast/extra.jsonl")
}
