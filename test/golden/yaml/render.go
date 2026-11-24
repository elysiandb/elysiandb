package main

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/taymour/elysiandb/internal/configuration"
)

func renderString(s string) map[string]any {
	if utf8.ValidString(s) {
		return map[string]any{"t": "str", "v": s}
	}
	return map[string]any{"t": "str", "hex": hex.EncodeToString([]byte(s))}
}

func mustJSON(v any) []byte {
	b, err := json.Marshal(v)
	if err != nil {
		panic(err)
	}
	return b
}

func render(v reflect.Value) any {
	switch v.Kind() {
	case reflect.Interface, reflect.Ptr:
		if v.IsNil() {
			return map[string]any{"t": "null"}
		}
		return render(v.Elem())
	case reflect.Bool:
		return map[string]any{"t": "bool", "v": v.Bool()}
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return map[string]any{"t": "int", "v": strconv.FormatInt(v.Int(), 10)}
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		return map[string]any{"t": "uint", "v": strconv.FormatUint(v.Uint(), 10)}
	case reflect.Float32, reflect.Float64:
		return map[string]any{"t": "float", "v": fmt.Sprintf("%016x", math.Float64bits(v.Float()))}
	case reflect.String:
		return renderString(v.String())
	case reflect.Slice:
		if v.IsNil() {
			return map[string]any{"t": "nilseq"}
		}
		items := make([]any, v.Len())
		for i := range items {
			items[i] = render(v.Index(i))
		}
		return map[string]any{"t": "seq", "v": items}
	case reflect.Map:
		type entry struct {
			k, v any
			kj   []byte
			vj   []byte
		}
		entries := make([]entry, 0, v.Len())
		iter := v.MapRange()
		for iter.Next() {
			k := render(iter.Key())
			val := render(iter.Value())
			entries = append(entries, entry{k, val, mustJSON(k), mustJSON(val)})
		}
		sort.Slice(entries, func(i, j int) bool {
			if c := bytes.Compare(entries[i].kj, entries[j].kj); c != 0 {
				return c < 0
			}
			return bytes.Compare(entries[i].vj, entries[j].vj) < 0
		})
		pairs := make([]any, len(entries))
		for i, e := range entries {
			pairs[i] = []any{e.k, e.v}
		}
		return map[string]any{"t": "map", "v": pairs}
	case reflect.Struct:
		t := v.Type()
		pairs := make([]any, 0, t.NumField())
		for i := 0; i < t.NumField(); i++ {
			pairs = append(pairs, []any{renderString(yamlKey(t.Field(i))), render(v.Field(i))})
		}
		return map[string]any{"t": "map", "v": pairs}
	}
	panic("cannot render " + v.Type().String())
}

func yamlKey(f reflect.StructField) string {
	if key := f.Tag.Get("yaml"); key != "" {
		return key
	}
	return strings.ToLower(f.Name)
}

func leavesOf(v reflect.Value, prefix string, out *[][2]any) {
	t := v.Type()
	for i := 0; i < t.NumField(); i++ {
		path := yamlKey(t.Field(i))
		if prefix != "" {
			path = prefix + "." + path
		}
		f := v.Field(i)
		if f.Kind() == reflect.Struct {
			leavesOf(f, path, out)
			continue
		}
		*out = append(*out, [2]any{path, render(f)})
	}
}

func configDiff(v reflect.Value) []any {
	var base, got [][2]any
	leavesOf(reflect.ValueOf(configuration.DefaultConfig()), "", &base)
	leavesOf(v, "", &got)
	diff := []any{}
	for i := range got {
		if !bytes.Equal(mustJSON(base[i][1]), mustJSON(got[i][1])) {
			diff = append(diff, []any{got[i][0], got[i][1]})
		}
	}
	return diff
}

func canon(v any) string {
	return string(mustJSON(v))
}
