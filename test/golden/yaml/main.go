package main

import (
	"bytes"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"reflect"
	"unicode/utf8"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/test/golden/goldenio"
	"gopkg.in/yaml.v2"
)

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("yaml", run)
}

type ycase struct {
	name    string
	input   []byte
	targets []string
	shallow bool
}

var targetTypes = map[string]reflect.Type{
	"iface":   reflect.TypeOf((*interface{})(nil)).Elem(),
	"int":     reflect.TypeOf(int(0)),
	"bool":    reflect.TypeOf(false),
	"string":  reflect.TypeOf(""),
	"strings": reflect.TypeOf([]string(nil)),
	"optbool": reflect.TypeOf((*bool)(nil)),
	"struct":  reflect.TypeOf(probe{}),
	"config":  reflect.TypeOf(configuration.Config{}),
}

type probeInner struct {
	X int      `yaml:"x"`
	Y []string `yaml:"y"`
}

type probe struct {
	A int          `yaml:"a"`
	B string       `yaml:"b"`
	C bool         `yaml:"c"`
	D []string     `yaml:"d"`
	E *bool        `yaml:"e"`
	F probeInner   `yaml:"f"`
	G []probeInner `yaml:"g"`
	H interface{}  `yaml:"h"`
}

func run(out string) error {
	files := []struct {
		name  string
		cases func() []ycase
	}{
		{"scalars.jsonl", scalarCases},
		{"documents.jsonl", documentCases},
		{"syntax.jsonl", syntaxCases},
		{"config.jsonl", configCases},
		{"deep.jsonl", deepCases},
	}
	for _, f := range files {
		var l goldenio.Lines
		seen := map[string]bool{}
		for _, c := range f.cases() {
			if seen[c.name] {
				return fmt.Errorf("%s: duplicate case name %q", f.name, c.name)
			}
			seen[c.name] = true
			row, err := runCase(c)
			if err != nil {
				return fmt.Errorf("%s: %s: %w", f.name, c.name, err)
			}
			l.Add(row)
		}
		if err := l.Write(out, f.name); err != nil {
			return err
		}
	}
	return writeConfigKeys(out)
}

func writeConfigKeys(out string) error {
	var l goldenio.Lines
	leaves, sections := configKeys()
	for _, s := range sections {
		l.Add(map[string]any{"path": s, "kind": "section"})
	}
	var defaults [][2]any
	leavesOf(reflect.ValueOf(configuration.DefaultConfig()), "", &defaults)
	for i, k := range leaves {
		l.Add(map[string]any{"path": k.path, "kind": k.kind, "default": defaults[i][1]})
	}
	return l.Write(out, "config_keys.jsonl")
}

func putText(row map[string]any, field string, b []byte) {
	if utf8.Valid(b) {
		row[field] = string(b)
	} else {
		row[field+"_hex"] = hex.EncodeToString(b)
	}
}

func runCase(c ycase) (map[string]any, error) {
	row := map[string]any{"name": c.name}
	putText(row, "input", c.input)
	parse, perr := parseCase(c.input, c.shallow)
	if perr != nil {
		return nil, perr
	}
	row["parse"] = parse.data
	if parse.failed {
		for _, t := range []string{"iface", "struct", "config", "int"} {
			v := reflect.New(targetTypes[t])
			err := yaml.Unmarshal(c.input, v.Interface())
			if err == nil || err.Error() != parse.msg {
				return nil, fmt.Errorf("target %s: parse error %q, unmarshal gave %v", t, parse.msg, err)
			}
		}
		return row, nil
	}
	targets := map[string]any{}
	for _, t := range c.targets {
		res, err := decodeTarget(c, parse, t)
		if err != nil {
			return nil, fmt.Errorf("target %s: %w", t, err)
		}
		targets[t] = res
	}
	row["targets"] = targets
	return row, nil
}

func decodeTarget(c ycase, parse parsed, target string) (map[string]any, error) {
	strict := false
	name := target
	if n, ok := cutSuffix(target, "-strict"); ok {
		strict = true
		name = n
	}
	typ, ok := targetTypes[name]
	if !ok {
		return nil, fmt.Errorf("unknown target %s", target)
	}
	v := reflect.New(typ)
	if name == "config" && !strict {
		v.Elem().Set(reflect.ValueOf(configuration.DefaultConfig()))
	}
	var err error
	if strict {
		err = yaml.UnmarshalStrict(c.input, v.Interface())
	} else {
		err = yaml.Unmarshal(c.input, v.Interface())
	}
	r := newReplay(strict)
	want := r.run(parse.doc, typ)
	got := ""
	if err != nil {
		got = err.Error()
	}
	if want.prefix {
		if len(got) < len(want.msg) || got[:len(want.msg)] != want.msg {
			return nil, fmt.Errorf("replay predicted prefix %q, yaml.v2 gave %q", want.msg, got)
		}
	} else if got != want.msg {
		return nil, fmt.Errorf("replay predicted %q, yaml.v2 gave %q", want.msg, got)
	}
	res := map[string]any{}
	if want.fatal != nil {
		res["fatal"] = want.fatal
		return res, nil
	}
	res["errors"] = want.issues
	if strict || c.shallow {
		return res, nil
	}
	if name == "config" {
		res["diff"] = configDiff(v.Elem())
	} else {
		res["value"] = render(v.Elem())
	}
	if name == "iface" && err == nil {
		dec := yaml.NewDecoder(bytes.NewReader(nonEmpty(c.input)))
		var w interface{}
		derr := dec.Decode(&w)
		if derr != nil && derr != io.EOF {
			return nil, fmt.Errorf("decoder: %v", derr)
		}
		if !reflect.DeepEqual(canon(render(reflect.ValueOf(&w).Elem())), canon(res["value"])) {
			return nil, errors.New("decoder and unmarshal disagree")
		}
	}
	return res, nil
}

func cutSuffix(s, suffix string) (string, bool) {
	if len(s) >= len(suffix) && s[len(s)-len(suffix):] == suffix {
		return s[:len(s)-len(suffix)], true
	}
	return s, false
}

func nonEmpty(b []byte) []byte {
	if len(b) == 0 {
		return []byte{'\n'}
	}
	return b
}
