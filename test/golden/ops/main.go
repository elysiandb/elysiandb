package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/test/golden/goldenio"
	"gopkg.in/yaml.v2"
)

func main() {
	out := flag.String("out", "", "tests/golden/ops")
	only := flag.String("only", "", "comma-separated steps to run (default all)")
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "missing -out")
		os.Exit(2)
	}
	steps := []struct {
		name string
		fn   func(string) error
	}{{"yaml", genYAML}, {"config", genConfig}, {"logs", genLogs}, {"fmt", genFmt}, {"metrics", genMetrics}, {"cli", genCLI}}
	for _, s := range steps {
		if *only != "" && !strings.Contains(","+*only+",", ","+s.name+",") {
			continue
		}
		if err := s.fn(*out); err != nil {
			fmt.Fprintf(os.Stderr, "%s: %v\n", s.name, err)
			os.Exit(1)
		}
	}
}

type yamlCase struct {
	name    string
	input   string
	unknown []string
	dupes   []string
	key     string
}

var lineRe = regexp.MustCompile(`line (\d+):`)

func loadCase(dir string, c yamlCase) (map[string]any, error) {
	path := filepath.Join(dir, "config.yaml")
	if err := os.WriteFile(path, []byte(c.input), 0o644); err != nil {
		return nil, err
	}
	var logs bytes.Buffer
	log.SetOutput(&logs, false)
	cfg, err := configuration.LoadConfig(path)
	row := map[string]any{"name": c.name, "input": c.input, "ok": err == nil, "error": nil}
	if c.unknown != nil {
		row["unknown_keys"] = c.unknown
	}
	if c.dupes != nil {
		row["duplicate_keys"] = c.dupes
	}
	if strings.Contains(logs.String(), "unknown configuration keys are ignored") {
		row["warned_unknown_keys"] = true
	}
	if err != nil {
		row["error"] = classify(err, c.key)
	} else {
		b, merr := json.Marshal(*cfg)
		if merr != nil {
			return nil, merr
		}
		row["config"] = string(b)
	}
	return row, nil
}

func classify(err error, key string) map[string]any {
	msg := err.Error()
	var te *yaml.TypeError
	if errors.As(err, &te) {
		var lines []string
		for _, e := range te.Errors {
			if m := lineRe.FindStringSubmatch(e); m != nil {
				lines = append(lines, m[1])
			}
		}
		return map[string]any{"class": "yaml-type", "literal": "cannot parse the configuration file: ", "lines": lines, "key": key, "count": len(te.Errors)}
	}
	if rest, ok := strings.CutPrefix(msg, "cannot parse the configuration file: "); ok {
		out := map[string]any{"class": "yaml-syntax", "literal": "cannot parse the configuration file: "}
		if m := lineRe.FindStringSubmatch(rest); m != nil {
			out["line"] = m[1]
		}
		return out
	}
	var pe *fs.PathError
	if errors.As(err, &pe) {
		return map[string]any{"class": "os", "literal": "cannot read the configuration file: "}
	}
	return map[string]any{"class": "validation", "message": msg}
}

func yamlCases() []yamlCase {
	var cs []yamlCase
	add := func(name, input string) { cs = append(cs, yamlCase{name: name, input: input}) }
	add("empty", "")
	add("comment-only", "# nothing\n")
	add("minimal", "store:\n  folder: /data\n")
	for _, v := range []string{"y", "Y", "yes", "Yes", "YES", "true", "True", "TRUE", "on", "On", "ON", "n", "N", "no", "No", "NO", "false", "False", "FALSE", "off", "Off", "OFF", `"true"`, "1", "0", "~", "null", "!!bool yes", "!!str true"} {
		cs = append(cs, yamlCase{name: "bool " + v, input: "stats:\n  enabled: " + v + "\n", key: "stats.enabled"})
	}
	for _, v := range []string{"8", "010", "0o10", "0x10", "0b101", "-3", "+3", "1_000", "1.5", "1e3", "99999999999999999999", "18446744073709551615", "9223372036854775807", "-9223372036854775808", `"8"`, "abc", "~", "!!int 12", "!!float 3", "[]", "{}", ".inf", ".nan"} {
		cs = append(cs, yamlCase{name: "int " + v, input: "store:\n  shards: " + v + "\n", key: "store.shards"})
	}
	for _, v := range []string{"100", "-5", "1.5", "2e3", `"7"`, "0x1F", "~"} {
		cs = append(cs, yamlCase{name: "int64 " + v, input: "store:\n  memory:\n    limitMB: " + v + "\n", key: "store.memory.limitMB"})
	}
	for _, v := range []string{"/data", `"/data dir"`, "'single'", "2026-01-01", "2026-01-01T10:00:00Z", "12", "true", "~", "|\n    multi\n    line", ">\n    folded\n    text", `"esc\taped\u00e9"`, "!!str 12", "[a, b]"} {
		cs = append(cs, yamlCase{name: "string " + strings.ReplaceAll(v, "\n", " "), input: "store:\n  folder: " + v + "\n", key: "store.folder"})
	}
	for _, v := range []string{"[a, b]", "\n    - a\n    - b", "[]", "~", "a", "[1, 2]", "[[a]]", "{a: b}"} {
		cs = append(cs, yamlCase{name: "list " + strings.ReplaceAll(v, "\n", " "), input: "server:\n  http:\n    trustedProxies: " + v + "\n", key: "server.http.trustedProxies"})
	}
	for _, v := range []string{"true", "false", "~", "maybe"} {
		cs = append(cs, yamlCase{name: "pointer bool " + v, input: "api:\n  query:\n    cancelOnDisconnect: " + v + "\n", key: "api.query.cancelOnDisconnect"})
	}
	add("server scalar", "server: 3\n")
	add("server sequence", "server:\n  - a\n")
	add("port mapping", "server:\n  http:\n    port:\n      a: 1\n")
	add("top sequence", "- a\n- b\n")
	add("top scalar", "hello\n")
	add("anchors and merge", "base: &b\n  enabled: true\n  port: 9000\nserver:\n  http:\n    <<: *b\n    host: h\n")
	add("merge then override", "base: &b\n  port: 9000\nserver:\n  http:\n    port: 1\n    <<: *b\n")
	cs = append(cs, yamlCase{name: "duplicate keys", input: "store:\n  shards: 4\n  shards: 8\n", dupes: []string{"store.shards"}})
	cs = append(cs, yamlCase{name: "duplicate sections", input: "store:\n  shards: 4\nstore:\n  folder: /x\n", dupes: []string{"store"}})
	cs = append(cs, yamlCase{name: "unknown top", input: "nope: 1\n", unknown: []string{"nope"}})
	cs = append(cs, yamlCase{name: "unknown nested", input: "store:\n  json: 1\nsecurity:\n  authentication:\n    enabled: false\n  extra: x\n", unknown: []string{"store.json", "security.extra"}})
	cs = append(cs, yamlCase{name: "unknown pprof", input: "monitoring:\n  pprof:\n    enabled: true\n"})
	cs = append(cs, yamlCase{name: "unknown deep", input: "api:\n  cache:\n    enabled: true\n    size: 3\n", unknown: []string{"api.cache.size"}})
	for _, bad := range []struct{ name, input string }{
		{"unclosed flow", "store: [this is: not: valid\n"},
		{"unclosed quote", "store:\n  folder: \"abc\n"},
		{"bad indentation", "store:\n  folder: a\n folder2: b\n"},
		{"tab indentation", "store:\n\tfolder: a\n"},
		{"invalid escape", "store:\n  folder: \"\\q\"\n"},
		{"unknown anchor", "store: *nope\n"},
		{"recursive anchor", "a: &a [*a]\n"},
		{"bom", "\ufeffstore:\n  folder: /x\n"},
		{"utf16", "\xff\xfes\x00t\x00o\x00r\x00e\x00:\x00 \x00{\x00}\x00\n\x00"},
		{"colon in key", "store:\n  fold:er: a\n"},
		{"block in flow", "store: {a: - b}\n"},
		{"trailing garbage", "store: {}\n}\n"},
		{"document separators", "store:\n  shards: 2\n---\nstore:\n  shards: 3\n"},
	} {
		add(bad.name, bad.input)
	}
	for _, v := range []struct{ name, input string }{
		{"log level bad", "log:\n  level: loud\n"},
		{"log format bad", "log:\n  format: xml\n"},
		{"slow request negative", "log:\n  slowRequestMs: -1\n"},
		{"buffer lines negative", "log:\n  bufferLines: -1\n"},
		{"engine bad", "engine:\n  name: mongodb\n"},
		{"durability bad", "store:\n  durability: always\n"},
		{"durability spaced", "store:\n  durability: \" fsync \"\n"},
		{"durability upper", "store:\n  durability: FSYNC\n"},
		{"read timeout negative", "server:\n  http:\n    readTimeoutSeconds: -1\n"},
		{"query workers negative", "api:\n  query:\n    workers: -2\n"},
		{"spool negative", "api:\n  response:\n    spoolMB: -1\n"},
		{"memory negative", "store:\n  memory:\n    limitMB: -1\n"},
		{"max queued -1", "api:\n  query:\n    maxQueued: -1\n"},
		{"max queued -2", "api:\n  query:\n    maxQueued: -2\n"},
		{"auth token missing", "security:\n  authentication:\n    enabled: true\n    mode: token\n"},
		{"auth mode upper", "security:\n  authentication:\n    enabled: true\n    mode: \" USER \"\n"},
		{"auth mode bad", "security:\n  authentication:\n    enabled: true\n    mode: ldap\n"},
		{"auth disabled bad mode", "security:\n  authentication:\n    enabled: false\n    mode: ldap\n"},
	} {
		add(v.name, v.input)
	}
	for _, name := range []string{"elysian.yaml", "docker/elysian.yaml"} {
		if b, err := os.ReadFile(name); err == nil {
			add("shipped "+name, string(b))
		}
	}
	return cs
}

func genYAML(out string) error {
	dir, err := os.MkdirTemp("", "elysiandb-golden-yaml-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(dir)
	var l goldenio.Lines
	for _, c := range yamlCases() {
		row, err := loadCase(dir, c)
		if err != nil {
			return fmt.Errorf("%s: %w", c.name, err)
		}
		l.Add(row)
	}
	_, err = configuration.LoadConfig(filepath.Join(dir, "absent.yaml"))
	l.Add(map[string]any{"name": "missing file", "ok": false, "error": classify(err, "")})
	return l.Write(out, "yaml/cases.jsonl")
}

func genConfig(out string) error {
	dir, err := os.MkdirTemp("", "elysiandb-golden-config-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(dir)
	def := configuration.DefaultConfig()
	if err := goldenio.WriteFile(out, "config/default.json", []byte(def.ToJson())); err != nil {
		return err
	}
	n := 0
	for _, c := range yamlCases() {
		if n >= 20 {
			break
		}
		path := filepath.Join(dir, "c.yaml")
		if err := os.WriteFile(path, []byte(c.input), 0o644); err != nil {
			return err
		}
		cfg, err := configuration.LoadConfig(path)
		if err != nil {
			continue
		}
		cfg.Security.Authentication.Token = "secret-token"
		cfg.Monitoring.Prometheus.Token = "metrics-token"
		n++
		if err := goldenio.WriteFile(out, fmt.Sprintf("config/%02d.json", n), []byte(cfg.ToJson())); err != nil {
			return err
		}
		if err := goldenio.WriteFile(out, fmt.Sprintf("config/%02d.yaml", n), []byte(c.input)); err != nil {
			return err
		}
	}
	return nil
}
