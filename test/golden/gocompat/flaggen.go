package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"strconv"
	"time"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

type flagDef struct {
	kind string
	name string
	def  any
}

var flagSets = []struct {
	name string
	defs []flagDef
}{
	{"elysiandb", []flagDef{{"string", "config", "elysian.yaml"}}},
	{"bench", []flagDef{
		{"bool", "pair", false},
		{"string", "addr", "127.0.0.1:8088"},
		{"int", "vus", 4},
		{"duration", "duration", 20 * time.Second},
		{"int", "rps", 0},
		{"int", "keys", 20000},
		{"float64", "setratio", 0.10},
		{"int", "payload", 16},
		{"bool", "warmup", true},
		{"bool", "live", true},
		{"string", "auth", ""},
	}},
}

type flagFailure struct {
	name  string
	value string
}

type recValue struct {
	flag.Value
	name   string
	failed *flagFailure
}

func (r recValue) Set(s string) error {
	err := r.Value.Set(s)
	if err != nil && r.failed.name == "" {
		r.failed.name = r.name
		r.failed.value = s
	}
	return err
}

func (r recValue) IsBoolFlag() bool {
	b, ok := r.Value.(interface{ IsBoolFlag() bool })
	return ok && b.IsBoolFlag()
}

func newFlagSet(defs []flagDef, failed *flagFailure) (*flag.FlagSet, map[string]flag.Getter) {
	tmp := flag.NewFlagSet("tmp", flag.ContinueOnError)
	for _, d := range defs {
		switch d.kind {
		case "bool":
			tmp.Bool(d.name, d.def.(bool), "")
		case "string":
			tmp.String(d.name, d.def.(string), "")
		case "int":
			tmp.Int(d.name, d.def.(int), "")
		case "float64":
			tmp.Float64(d.name, d.def.(float64), "")
		case "duration":
			tmp.Duration(d.name, d.def.(time.Duration), "")
		}
	}
	fs := flag.NewFlagSet("golden", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	getters := map[string]flag.Getter{}
	for _, d := range defs {
		inner := tmp.Lookup(d.name).Value
		getters[d.name] = inner.(flag.Getter)
		fs.Var(recValue{Value: inner, name: d.name, failed: failed}, d.name, "")
	}
	return fs, getters
}

func flagValueText(v any) string {
	switch x := v.(type) {
	case bool:
		return strconv.FormatBool(x)
	case string:
		return hs(x)
	case int:
		return strconv.Itoa(x)
	case float64:
		return fbits(x)
	case time.Duration:
		return strconv.FormatInt(int64(x), 10)
	}
	panic(fmt.Sprintf("unexpected flag value %T", v))
}

func flagName(arg string) string {
	name := arg[1:]
	if len(name) > 0 && name[0] == '-' {
		name = name[1:]
	}
	for i := 1; i < len(name); i++ {
		if name[i] == '=' {
			return name[:i]
		}
	}
	return name
}

func flagReason(kind, value string) (string, error) {
	var err error
	switch kind {
	case "bool":
		_, err = strconv.ParseBool(value)
	case "int":
		_, err = strconv.ParseInt(value, 0, strconv.IntSize)
	case "float64":
		_, err = strconv.ParseFloat(value, 64)
	case "duration":
		if _, err = time.ParseDuration(value); err != nil {
			return "syntax", nil
		}
	}
	if err == nil {
		return "", fmt.Errorf("flag value %q of kind %s parses again", value, kind)
	}
	return errKind(err), nil
}

func flagCase(set string, defs []flagDef, args []string) (map[string]any, error) {
	failed := &flagFailure{}
	fs, getters := newFlagSet(defs, failed)
	err := fs.Parse(args)
	rest := fs.Args()
	row := map[string]any{"set": set, "args": hexList(args)}
	if err == nil {
		values := map[string]string{}
		for name, g := range getters {
			values[name] = flagValueText(g.Get())
		}
		row["values"] = values
		row["rest"] = hexList(rest)
		return row, nil
	}
	consumed := len(args) - len(rest)
	switch {
	case errors.Is(err, flag.ErrHelp):
		row["error"] = "help"
	case failed.name != "":
		kind := ""
		for _, d := range defs {
			if d.name == failed.name {
				kind = d.kind
			}
		}
		reason, rerr := flagReason(kind, failed.value)
		if rerr != nil {
			return nil, rerr
		}
		row["error"] = "invalid"
		row["name"] = hs(failed.name)
		row["value"] = hs(failed.value)
		row["reason"] = reason
	case consumed < len(args) && err.Error() == fmt.Sprintf("bad flag syntax: %s", args[consumed]):
		row["error"] = "syntax"
		row["arg"] = hs(args[consumed])
	case consumed > 0 && err.Error() == fmt.Sprintf("flag provided but not defined: -%s", flagName(args[consumed-1])):
		row["error"] = "unknown"
		row["name"] = hs(flagName(args[consumed-1]))
	case consumed > 0 && err.Error() == fmt.Sprintf("flag needs an argument: -%s", flagName(args[consumed-1])):
		row["error"] = "missing"
		row["name"] = hs(flagName(args[consumed-1]))
	default:
		return nil, fmt.Errorf("flag error of %q not replayed", args)
	}
	return row, nil
}

var elysiandbFlagCases = [][]string{
	{},
	{"server"},
	{"-config", "c.yaml"},
	{"-config=c.yaml"},
	{"--config", "c.yaml"},
	{"--config=c.yaml"},
	{"-config=", "x"},
	{"-config", ""},
	{"-config"},
	{"--config"},
	{"-config", "-x"},
	{"-config", "--"},
	{"-config", "-h"},
	{"--", "-config", "x"},
	{"-", "-config", "x"},
	{"-config", "a", "-config", "b"},
	{"-config", "a", "backup", "--output", "x"},
	{"-config=a=b"},
	{"-config==a"},
	{"-config", "\xff\xfe"},
	{"-config", "a", "--", "b"},
	{"-config", "a", "-", "b"},
	{"x", "-config", "y"},
	{"", "-config", "y"},
	{"-h"},
	{"-help"},
	{"--help"},
	{"--h"},
	{"-help=1"},
	{"-h=false"},
	{"-h", "-x"},
	{"-config", "a", "-h"},
	{"-x"},
	{"-x", "-h"},
	{"--x=1"},
	{"-zzz"},
	{"-Config", "x"},
	{"-conf", "x"},
	{"-config\xff", "x"},
	{"-é"},
	{"---config", "x"},
	{"-=x"},
	{"--=x"},
	{"-"},
	{"--"},
	{"---"},
	{"--", "--"},
	{"-config", "a", "---"},
	{"-config", "a", "-=b"},
}

var benchFlagCases = [][]string{
	{},
	{"-pair"},
	{"--pair"},
	{"-pair=false"},
	{"-pair=0"},
	{"-pair=1"},
	{"-pair=T"},
	{"-pair=f"},
	{"-pair=TRUE"},
	{"-pair=True"},
	{"-pair=tRue"},
	{"-pair=maybe"},
	{"-pair="},
	{"-pair", "false"},
	{"-pair", "--", "x"},
	{"-live=false", "-warmup=0"},
	{"-vus", "8"},
	{"-vus=8"},
	{"-vus", "0x10"},
	{"-vus", "0X1f"},
	{"-vus", "010"},
	{"-vus", "0b11"},
	{"-vus", "0o17"},
	{"-vus", "0_10"},
	{"-vus", "1_000"},
	{"-vus", "1__000"},
	{"-vus", "_1"},
	{"-vus", "1_"},
	{"-vus", "9223372036854775807"},
	{"-vus", "9223372036854775808"},
	{"-vus", "-9223372036854775808"},
	{"-vus", "-9223372036854775809"},
	{"-vus", "0x8000000000000000"},
	{"-vus", "abc"},
	{"-vus", ""},
	{"-vus", "1.5"},
	{"-vus", " 1"},
	{"-vus", "-3"},
	{"-vus", "+3"},
	{"-vus", "08"},
	{"-vus", "0x"},
	{"-vus"},
	{"-vus", "-vus"},
	{"-vus", "\xff"},
	{"-setratio", "0.5"},
	{"-setratio", "1e400"},
	{"-setratio", "-1e400"},
	{"-setratio", "1e-400"},
	{"-setratio", "x"},
	{"-setratio", "0x1p-2"},
	{"-setratio", "0x1.8"},
	{"-setratio", "inf"},
	{"-setratio", "-Infinity"},
	{"-setratio", "NaN"},
	{"-setratio", "1_0.5"},
	{"-setratio", "1_000.5"},
	{"-setratio", ".5"},
	{"-setratio", "5."},
	{"-setratio", ""},
	{"-setratio", "1e"},
	{"-duration", "1m30s"},
	{"-duration", "1.5h"},
	{"-duration", "250ms"},
	{"-duration", "1us"},
	{"-duration", "1µs"},
	{"-duration", "1μs"},
	{"-duration", "x"},
	{"-duration", "10"},
	{"-duration", "0"},
	{"-duration", "+0"},
	{"-duration", "-1s"},
	{"-duration", "9999999999h"},
	{"-duration", "2562047h47m16.854775807s"},
	{"-duration", "2562047h47m16.854775808s"},
	{"-duration", ".5s"},
	{"-duration", "5.s"},
	{"-duration", "1h2m3s4ms5us6ns"},
	{"-duration", ""},
	{"-duration", "1d"},
	{"-addr", "h:1", "-auth", "a b"},
	{"-auth", "", "-addr="},
	{"-keys", "5", "-payload", "-1"},
	{"-rps", "100", "x", "-vus", "3"},
	{"-rps", "100", "-", "-vus", "3"},
	{"-vus", "1", "-vus", "2"},
	{"-vus", "1", "-vus", "x"},
	{"-vus", "x", "-zzz"},
	{"-zzz", "-vus", "x"},
	{"-h"},
	{"-help"},
	{"--help"},
	{"-zzz"},
	{"-pair", "-h"},
	{"-pairs"},
	{"-pair=true=1"},
	{"-duration"},
	{"-setratio"},
	{"-addr"},
}

func genFlag(out string) error {
	var l goldenio.Lines
	for _, s := range flagSets {
		cases := elysiandbFlagCases
		if s.name == "bench" {
			cases = benchFlagCases
		}
		for _, args := range cases {
			row, err := flagCase(s.name, s.defs, args)
			if err != nil {
				return err
			}
			l.Add(row)
		}
	}
	return l.Write(out, "flag/cases.jsonl")
}
