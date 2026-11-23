package hook

import (
	"bufio"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/taymour/elysiandb/test/golden/gojadump"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func TestGenerateHooksPlanBytecode(t *testing.T) {
	root := os.Getenv("ELYSIANDB_GOLDEN_OUT")
	if root == "" {
		t.Skip("ELYSIANDB_GOLDEN_OUT is not set")
	}
	out := filepath.Join(root, "hooks")
	f, err := os.Open(filepath.Join("..", "..", "test", "golden", "hooks", "cases.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 1<<20), 1<<24)
	var index goldenio.Lines
	for sc.Scan() {
		var r struct {
			Label  string `json:"label"`
			Script string `json:"script"`
		}
		if err := json.Unmarshal(sc.Bytes(), &r); err != nil {
			t.Fatal(err)
		}
		plan := buildPlan(r.Script, "postRead")
		if plan == nil {
			index.Add(map[string]any{"label": r.Label, "planned": false})
			continue
		}
		var ids []string
		for _, f := range plan.folds {
			ids = append(ids, f.id)
		}
		name := "bytecode/" + r.Label + ".plan.code"
		if err := goldenio.WriteFile(out, name, []byte(gojadump.Program(plan.program))); err != nil {
			t.Fatal(err)
		}
		index.Add(map[string]any{"label": r.Label, "planned": true, "folds": len(plan.folds), "fold_ids": ids, "file": name})
	}
	if err := sc.Err(); err != nil {
		t.Fatal(err)
	}
	if err := index.Write(out, "bytecode/plans.jsonl"); err != nil {
		t.Fatal(err)
	}
}
