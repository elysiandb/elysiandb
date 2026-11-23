package main

import (
	"errors"
	"fmt"
	"math"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func fmtArgs() []any {
	args := []any{
		nil, true, false, 0, 1, -1, 42, int64(math.MaxInt64), int64(math.MinInt64), uint64(math.MaxUint64), uint32(7), int32(-7), uint8(255),
		"", "abc", "a\"b\\c", "\xffx", "\u00e9\u2028", "%d", []byte("bytes"), []byte{0, 0xff}, []byte(nil),
		[]string{"a", "b c"}, []string(nil), []any{1.0, "x", nil, []any{true}}, []any{}, []any(nil),
		map[string]any{"b": 1.0, "a": []any{"x"}, "c": map[string]any{"z": nil}}, map[string]any{}, map[string]any(nil),
		map[string]int{"y": 2, "x": 1}, map[string]string{"k": "v"},
		time.Duration(0), time.Duration(1500) * time.Millisecond, time.Duration(-90) * time.Second, time.Duration(math.MaxInt64),
		errors.New("boom"), fmt.Errorf("wrap: %w", errors.New("inner")),
		struct {
			A int
			B string
		}{1, "x"},
	}
	for i, f := range floatCorpus() {
		if i%4 == 0 {
			args = append(args, f)
		}
	}
	args = append(args, math.NaN(), float32(0.1), float32(1e21))
	return args
}

var fmtVerbs = []string{"%v", "%s", "%d", "%q", "%x", "%X", "%g", "%+v", "%06d", "%016d", "%02X", "%-63s|", "%b", "% v", "% s", "%T", "%e", "%f", "%.2f", "%.3g", "%5.1f", "%t", "%c", "%U", "%#v"}

func genFmt(out string) error {
	var l goldenio.Lines
	for _, a := range fmtArgs() {
		row := map[string]any{"arg": fmtDesc(a)}
		outs := map[string]any{}
		for _, verb := range fmtVerbs {
			s := fmt.Sprintf(verb, a)
			if verb == "%T" || verb == "%#v" || strings.Contains(s, "%!") {
				continue
			}
			if utf8.ValidString(s) {
				outs[verb] = s
			} else {
				outs[verb+"_hex"] = hs(s)
			}
		}
		row["out"] = outs
		l.Add(row)
	}
	multi := []struct {
		format string
		args   []any
	}{
		{"%s=%d", []any{"a", 1}},
		{"%v|%v", []any{[]any{1.5, "s"}, map[string]any{"k": 1e21}}},
		{"%*d", []any{5, 3}},
		{"%-*d|", []any{5, 3}},
		{"%[2]s %[1]s", []any{"a", "b"}},
	}
	for _, m := range multi {
		descs := make([]any, len(m.args))
		for i, a := range m.args {
			descs[i] = fmtDesc(a)
		}
		l.Add(map[string]any{"format": m.format, "args": descs, "sprintf": hs(fmt.Sprintf(m.format, m.args...)), "sprint": hs(fmt.Sprint(m.args...)), "sprintln": hs(fmt.Sprintln(m.args...))})
	}
	return l.Write(out, "fmt/cases.jsonl")
}

func fmtDesc(a any) any {
	switch x := a.(type) {
	case int32:
		return []any{"i32", i64s(int64(x))}
	case uint32:
		return []any{"u32", u64s(uint64(x))}
	case uint8:
		return []any{"u8", u64s(uint64(x))}
	case map[string]int:
		pairs := []any{}
		for _, k := range sortedKeys(x) {
			pairs = append(pairs, []any{hs(k), i64s(int64(x[k]))})
		}
		return []any{"map_int", pairs}
	case map[string]string:
		pairs := []any{}
		for _, k := range sortedKeys(x) {
			pairs = append(pairs, []any{hs(k), hs(x[k])})
		}
		return []any{"map_str", pairs}
	case time.Duration:
		return []any{"duration", i64s(int64(x))}
	case error:
		return []any{"error", hs(x.Error())}
	case struct {
		A int
		B string
	}:
		return []any{"struct_ab", i64s(int64(x.A)), hs(x.B)}
	}
	return desc(a)
}
