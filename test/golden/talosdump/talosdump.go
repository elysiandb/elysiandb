package talosdump

import (
	"fmt"
	"reflect"
	"sort"
	"strconv"
	"strings"

	"github.com/dop251/goja"
)

func Program(prog *goja.Program) string {
	var b strings.Builder
	dumpProgram(&b, reflect.ValueOf(prog), "")
	return b.String()
}

var programType = reflect.TypeOf((*goja.Program)(nil))

func dumpProgram(b *strings.Builder, p reflect.Value, indent string) {
	prog := p.Elem()
	fmt.Fprintf(b, "%sfunc: %s\n", indent, formatValue(prog.FieldByName("funcName"), nil, 0))
	fmt.Fprintf(b, "%scode:\n", indent)
	code := prog.FieldByName("code")
	for pc := 0; pc < code.Len(); pc++ {
		ins := code.Index(pc)
		for ins.Kind() == reflect.Interface {
			ins = ins.Elem()
		}
		var nested []reflect.Value
		fmt.Fprintf(b, "%s%d\t%s\t%s\n", indent, pc, ins.Type().String(), formatValue(ins, &nested, 0))
		for _, n := range nested {
			dumpProgram(b, n, indent+"  ")
		}
	}
	fmt.Fprintf(b, "%spositions:\n", indent)
	srcMap := prog.FieldByName("srcMap")
	for i := 0; i < srcMap.Len(); i++ {
		item := srcMap.Index(i)
		fmt.Fprintf(b, "%s%d\t%d\n", indent, item.FieldByName("pc").Int(), item.FieldByName("srcPos").Int())
	}
}

func formatValue(v reflect.Value, nested *[]reflect.Value, depth int) string {
	if depth > 12 {
		return "<deep>"
	}
	if !v.IsValid() {
		return "<nil>"
	}
	if t := v.Type(); v.Kind() == reflect.Struct || v.Kind() == reflect.Pointer {
		named := t
		if v.Kind() == reflect.Pointer {
			named = t.Elem()
		}
		if pkg := named.PkgPath(); pkg != "" && !strings.HasPrefix(pkg, "github.com/dop251/goja") {
			return "<" + named.String() + ">"
		}
	}
	switch v.Kind() {
	case reflect.Interface:
		if v.IsNil() {
			return "<nil>"
		}
		return formatValue(v.Elem(), nested, depth)
	case reflect.Pointer:
		if v.IsNil() {
			return "<nil>"
		}
		if v.Type() == programType {
			if nested != nil {
				*nested = append(*nested, v)
			}
			return "<prog>"
		}
		if v.Type().Elem().PkgPath() == "github.com/dop251/goja/file" {
			return "<file>"
		}
		return "&" + formatValue(v.Elem(), nested, depth+1)
	case reflect.Struct:
		parts := make([]string, 0, v.NumField())
		for i := 0; i < v.NumField(); i++ {
			parts = append(parts, v.Type().Field(i).Name+":"+formatValue(v.Field(i), nested, depth+1))
		}
		return "{" + strings.Join(parts, " ") + "}"
	case reflect.Slice:
		if v.IsNil() {
			return "[]"
		}
		fallthrough
	case reflect.Array:
		if v.Type().Elem().Kind() == reflect.Uint16 {
			runes := make([]rune, v.Len())
			for i := range runes {
				runes[i] = rune(v.Index(i).Uint())
			}
			return strconv.Quote(string(runes))
		}
		parts := make([]string, v.Len())
		for i := 0; i < v.Len(); i++ {
			parts[i] = formatValue(v.Index(i), nested, depth+1)
		}
		return "[" + strings.Join(parts, " ") + "]"
	case reflect.Map:
		keys := v.MapKeys()
		type kv struct{ k, v string }
		pairs := make([]kv, 0, len(keys))
		for _, k := range keys {
			pairs = append(pairs, kv{formatValue(k, nested, depth+1), formatValue(v.MapIndex(k), nested, depth+1)})
		}
		sort.Slice(pairs, func(i, j int) bool { return pairs[i].k < pairs[j].k })
		parts := make([]string, len(pairs))
		for i, p := range pairs {
			parts[i] = p.k + ":" + p.v
		}
		return "map[" + strings.Join(parts, " ") + "]"
	case reflect.String:
		return strconv.Quote(v.String())
	case reflect.Bool:
		return strconv.FormatBool(v.Bool())
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return strconv.FormatInt(v.Int(), 10)
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64, reflect.Uintptr:
		return strconv.FormatUint(v.Uint(), 10)
	case reflect.Float32, reflect.Float64:
		return strconv.FormatFloat(v.Float(), 'g', -1, 64)
	case reflect.Func:
		if v.IsNil() {
			return "<nil>"
		}
		return "<func>"
	case reflect.Chan:
		return "<chan>"
	}
	return "<" + v.Kind().String() + ">"
}
