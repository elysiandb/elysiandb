package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

const input = `{"id":"x","price":12,"ratio":0.5,"tags":["a","b"],"meta":{"k":"v","n":1},"nul":null,"s":"str","b":true,"big":12345678901234567890}`

var mapOrder = map[string]bool{"forin-entity": true, "json": true, "keys-order": true, "keys-order-2": true, "keys-order-3": true, "freeze": true}

var interruptAt = regexp.MustCompile(`(stopped after [0-9]+ms at [^ ]+ )\(<eval>:[0-9]+:[0-9]+\([0-9]+\)\)`)

var batchTwo = map[string]bool{"fn-names": true, "ctx-mutation": true, "filter-values": true, "fold-planned": true, "count-sum-types": true, "group-ops": true}

type caseRow struct {
	Label        string          `json:"label"`
	Script       string          `json:"script"`
	ExpectedRust json.RawMessage `json:"expected_rust,omitempty"`
}

func main() {
	out := flag.String("out", "", "tests/golden/hooks")
	zone := flag.String("zone", "", "utc or europe-paris")
	cases := flag.String("cases", "test/golden/hooks/cases.jsonl", "case table")
	compare := flag.String("compare", "", "corpus of the other architecture: write only the differences")
	flag.Parse()
	want := map[string]string{"utc": "UTC", "europe-paris": "Europe/Paris"}[*zone]
	if want == "" || os.Getenv("TZ") != want {
		fmt.Fprintf(os.Stderr, "run with -zone utc and TZ=UTC, or -zone europe-paris and TZ=Europe/Paris\n")
		os.Exit(2)
	}
	if err := run(*out, *zone, *cases, *compare); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func readCases(path string) ([]caseRow, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	var rows []caseRow
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 1<<20), 1<<24)
	for sc.Scan() {
		var r caseRow
		if err := json.Unmarshal(sc.Bytes(), &r); err != nil {
			return nil, err
		}
		rows = append(rows, r)
	}
	return rows, sc.Err()
}

func seed(label string) {
	switch {
	case strings.HasPrefix(label, "reflect-"):
		for i := 0; i < 2; i++ {
			api_storage.WriteEntity("order", map[string]any{"id": fmt.Sprintf("o%d", i), "group": "g"})
		}
	case batchTwo[label] || (strings.HasPrefix(label, "row-") && label != "row-keys"):
		for i := 0; i < 3; i++ {
			api_storage.WriteEntity("order", map[string]any{"id": fmt.Sprintf("o%d", i), "group": "g", "amount": float64(i) + 0.5})
		}
	default:
		for i := 0; i < 3; i++ {
			api_storage.WriteEntity("order", map[string]any{"id": fmt.Sprintf("o%d", i), "group": "g", "amount": float64(i) + 0.5, "tags": []any{"t"}, "owner": map[string]any{"n": "x"}})
		}
	}
}

func cleanPaths(s string) string {
	s = strings.ReplaceAll(s, "github.com/dop251/goja", "<goja-module>")
	s = strings.ReplaceAll(s, "github.com/taymour/elysiandb", "<module>")
	return s
}

func run(out, zone, casesPath, compare string) error {
	rows, err := readCases(casesPath)
	if err != nil {
		return err
	}
	tmp, err := os.MkdirTemp("", "elysiandb-golden-hooks-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(tmp)
	log.SetOutput(io.Discard, false)
	var lines goldenio.Lines
	var all []map[string]any
	for i, r := range rows {
		cfg := &configuration.Config{}
		cfg.Store.Folder = filepath.Join(tmp, fmt.Sprintf("s%03d", i))
		cfg.Store.Shards = 4
		cfg.Api.Hooks.Enabled = true
		cfg.Log.Level = "error"
		if strings.HasPrefix(r.Label, "timeout") {
			cfg.Api.Hooks.TimeoutMs = 100
		}
		globals.SetConfig(cfg)
		storage.LoadDB()
		storage.LoadJsonDB()
		cache.InitCache(30 * time.Second)
		hook.InitHooks()
		seed(r.Label)
		var entity map[string]any
		if err := json.Unmarshal([]byte(input), &entity); err != nil {
			return err
		}
		errText := ""
		if err := hook.ApplyPostReadScript("", r.Script, entity, true); err != nil {
			errText = err.Error()
		}
		output := ""
		if b, err := json.Marshal(entity); err != nil {
			output = "MARSHAL_ERROR: " + err.Error()
		} else {
			output = string(b)
		}
		row := map[string]any{"label": r.Label, "script": r.Script, "input": input, "error": cleanPaths(errText), "output": cleanPaths(output)}
		if mapOrder[r.Label] {
			row["error"] = nil
			row["output"] = nil
			row["go_map_order"] = true
		}
		if interruptAt.MatchString(errText) {
			row["error"] = interruptAt.ReplaceAllString(cleanPaths(errText), "${1}(<eval>:<position>)")
			row["masked"] = "interrupt position"
		}
		if len(r.ExpectedRust) > 0 {
			row["expected_rust"] = r.ExpectedRust
		}
		lines.Add(row)
		all = append(all, row)
		_ = storage.Close()
	}
	if compare == "" {
		return lines.Write(out, "corpus-"+zone+".jsonl")
	}
	return writeDifferences(out, zone, compare, all)
}

func writeDifferences(out, zone, compare string, rows []map[string]any) error {
	data, err := os.ReadFile(compare)
	if err != nil {
		return err
	}
	base := map[string]map[string]any{}
	for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		var r map[string]any
		if err := json.Unmarshal([]byte(line), &r); err != nil {
			return err
		}
		base[r["label"].(string)] = r
	}
	var lines goldenio.Lines
	for _, r := range rows {
		b := base[r["label"].(string)]
		e1, _ := r["error"].(string)
		e2, _ := b["error"].(string)
		if strings.Contains(e1, "stopped after") || strings.Contains(e2, "stopped after") {
			continue
		}
		if fmt.Sprint(r["error"]) == fmt.Sprint(b["error"]) && fmt.Sprint(r["output"]) == fmt.Sprint(b["output"]) {
			continue
		}
		lines.Add(map[string]any{"label": r["label"], "goarch": runtime.GOARCH, "error": r["error"], "output": r["output"]})
	}
	return lines.Write(out, "arch-"+runtime.GOARCH+"-"+zone+".jsonl")
}
