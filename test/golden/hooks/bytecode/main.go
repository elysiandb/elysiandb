package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"os"

	"github.com/dop251/goja"
	"github.com/taymour/elysiandb/test/golden/talosdump"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

type caseRow struct {
	Label  string `json:"label"`
	Script string `json:"script"`
}

func main() {
	out := flag.String("out", "", "tests/golden/hooks")
	cases := flag.String("cases", "test/golden/hooks/cases.jsonl", "case table")
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "missing -out")
		os.Exit(2)
	}
	if err := run(*out, *cases); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(out, casesPath string) error {
	f, err := os.Open(casesPath)
	if err != nil {
		return err
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 1<<20), 1<<24)
	var index goldenio.Lines
	for sc.Scan() {
		var r caseRow
		if err := json.Unmarshal(sc.Bytes(), &r); err != nil {
			return err
		}
		prog, err := goja.Compile("", r.Script, false)
		if err != nil {
			index.Add(map[string]any{"label": r.Label, "compiled": false})
			continue
		}
		if err := goldenio.WriteFile(out, "bytecode/"+r.Label+".code", []byte(talosdump.Program(prog))); err != nil {
			return err
		}
		index.Add(map[string]any{"label": r.Label, "compiled": true, "file": "bytecode/" + r.Label + ".code"})
	}
	if err := sc.Err(); err != nil {
		return err
	}
	return index.Write(out, "bytecode/index.jsonl")
}
