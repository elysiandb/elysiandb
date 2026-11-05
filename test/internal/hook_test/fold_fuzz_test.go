package hook_test

import (
	"fmt"
	"math/rand"
	"strings"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/hook"
)

type scriptGen struct {
	r      *rand.Rand
	locals []string
}

func (g *scriptGen) leaf() string {
	switch g.r.Intn(9) {
	case 0, 1, 2:
		return "p." + []string{"a", "b", "c"}[g.r.Intn(3)]
	case 3:
		return fmt.Sprint(g.r.Intn(5))
	case 4:
		return []string{`"x"`, `"2026-03"`, `""`, `"é"`, `"10"`}[g.r.Intn(5)]
	case 5:
		return "month"
	case 6:
		return []string{"null", "undefined", "true", "false", "1.5"}[g.r.Intn(5)]
	case 7:
		return "ctx.entity.adjust"
	}

	if len(g.locals) > 0 {
		return g.locals[g.r.Intn(len(g.locals))]
	}

	return "p.a"
}

func (g *scriptGen) expr(depth int) string {
	if depth <= 0 {
		return g.leaf()
	}

	switch g.r.Intn(12) {
	case 0, 1, 2:
		op := []string{"+", "-", "*", "/", "%"}[g.r.Intn(5)]
		return "(" + g.expr(depth-1) + " " + op + " " + g.expr(depth-1) + ")"
	case 3, 4, 5:
		op := []string{"===", "!==", "<", "<=", ">", ">=", "==", "!="}[g.r.Intn(8)]
		return "(" + g.expr(depth-1) + " " + op + " " + g.expr(depth-1) + ")"
	case 6:
		return "(" + g.expr(depth-1) + " || " + g.expr(depth-1) + ")"
	case 7:
		return "(" + g.expr(depth-1) + " && " + g.expr(depth-1) + ")"
	case 8:
		return "!" + g.expr(depth-1)
	case 9:
		return "(" + g.expr(depth-1) + " ? " + g.expr(depth-1) + " : " + g.expr(depth-1) + ")"
	case 10:
		return []string{"typeof " + g.expr(depth-1), "parseInt(" + g.expr(depth-1) + ", 10)", "(" + g.expr(depth-1) + ").length", "-" + g.expr(depth-1)}[g.r.Intn(4)]
	}

	return "first(" + g.expr(depth-1) + ")"
}

func (g *scriptGen) body() string {
	var b strings.Builder
	for i := 0; i < 1+g.r.Intn(5); i++ {
		switch g.r.Intn(6) {
		case 0:
			name := fmt.Sprintf("v%d", len(g.locals))
			fmt.Fprintf(&b, "    var %s = %s\n", name, g.expr(2))
			g.locals = append(g.locals, name)
		case 1:
			fmt.Fprintf(&b, "    if (%s) { continue }\n", g.expr(2))
		case 2:
			fmt.Fprintf(&b, "    total += %s\n", g.expr(2))
		case 3:
			fmt.Fprintf(&b, "    if (%s) { total -= %s } else { count++ }\n", g.expr(2), g.expr(1))
		case 4:
			fmt.Fprintf(&b, "    count++\n")
		case 5:
			fmt.Fprintf(&b, "    if (%s) { count -= 1 }\n", g.expr(1))
		}
	}

	return b.String()
}

func (g *scriptGen) script(filter string) string {
	g.locals = nil
	return fmt.Sprintf(`function first(s) {
  var parts = s.split("-")
  return parts[0]
}
function postRead(ctx) {
  var month = ctx.entity.id
  var rows = ctx.query("fuzz", %s)
  var total = 0, count = 0
  for (var i = 0; i < rows.length; i++) {
    var p = rows[i]
%s  }
  ctx.entity.total = String(total)
  ctx.entity.count = String(count)
}`, filter, g.body())
}

func fuzzDoc(r *rand.Rand, id string) map[string]any {
	value := func() any {
		switch r.Intn(11) {
		case 0:
			return nil
		case 1:
			return true
		case 2:
			return ""
		case 3:
			return "2026-03"
		case 4:
			return "10"
		case 5:
			return "é-x"
		case 6:
			return map[string]any{"n": 1.0}
		case 7:
			return []any{"a"}
		}
		return float64(r.Intn(1000)) / 8
	}

	doc := map[string]any{"id": id, "group": []string{"g1", "g2", "G1"}[r.Intn(3)]}
	for _, f := range []string{"a", "b", "c"} {
		if r.Intn(5) > 0 {
			doc[f] = value()
		}
	}

	return doc
}

func regularDoc(r *rand.Rand, id string) map[string]any {
	doc := map[string]any{"id": id, "group": []string{"g1", "g2", "G1"}[r.Intn(3)]}
	doc["a"] = float64(r.Intn(1000)) / 8
	doc["b"] = []string{"x", "2026-03", "10", ""}[r.Intn(4)]
	if r.Intn(3) > 0 {
		doc["c"] = float64(r.Intn(4))
	}

	return doc
}

func runBoth(t *testing.T, script string, id string) (string, string) {
	t.Helper()
	run := func(enabled bool) string {
		hook.ResetOutputMemo()
		hook.SetFoldsEnabled(enabled)
		defer hook.SetFoldsEnabled(true)

		out := map[string]any{"id": id, "adjust": 2.5}
		if err := hook.ApplyPostReadScript("", script, out, true); err != nil {
			return "error"
		}
		return fmt.Sprint(out["total"], "|", out["count"])
	}

	return run(true), run(false)
}

func TestFoldMatchesTheScriptOnRandomScripts(t *testing.T) {
	setup(t, true)
	if err := api_storage.CreateEntityType("fuzz"); err != nil {
		t.Fatal(err)
	}

	r := rand.New(rand.NewSource(20261105))
	seed := fuzzDoc
	for i := 0; i < 400; i++ {
		_ = api_storage.WriteEntity("fuzz", seed(r, fmt.Sprintf("d%04d", i)))
	}

	g := &scriptGen{r: r}
	filters := []string{`{}`, `{ group: { eq: "g1" } }`, `{ b: { neq: "x" } }`}

	check := func(label string, rounds int) {
		t.Helper()
		for i := 0; i < rounds; i++ {
			script := g.script(filters[r.Intn(len(filters))])
			fast, slow := runBoth(t, script, "2026-03")
			if fast != slow {
				t.Fatalf("%s: fold %q, script %q\n%s", label, fast, slow, script)
			}
		}
	}

	check("odd values", 200)

	for i := 0; i < 400; i++ {
		id := fmt.Sprintf("d%04d", i)
		api_storage.UpdateEntityById("fuzz", id, regularDoc(r, id))
	}

	before := hook.Folds()
	check("regular values", 300)
	if got := hook.Folds(); got.FullPasses == before.FullPasses {
		t.Fatalf("regular values should run on the view, stats %+v", got)
	}

	scripts := make2(g, filters, r, 30)
	for round := 0; round < 60; round++ {
		for _, script := range scripts {
			fast, slow := runBoth(t, script, "2026-03")
			if fast != slow {
				t.Fatalf("round %d: fold %q, script %q\n%s", round, fast, slow, script)
			}
		}

		id := fmt.Sprintf("d%04d", r.Intn(400))
		switch r.Intn(4) {
		case 0:
			api_storage.DeleteEntityById("fuzz", id)
		case 1:
			_ = api_storage.WriteEntity("fuzz", regularDoc(r, fmt.Sprintf("n%04d", round)))
		default:
			api_storage.UpdateEntityById("fuzz", id, regularDoc(r, id))
		}
	}

	if got := hook.Folds(); got.Incremental == 0 {
		t.Fatalf("writes should advance kept results, stats %+v", got)
	}
}

func make2(g *scriptGen, filters []string, r *rand.Rand, n int) []string {
	out := make([]string, 0, n)
	for i := 0; i < n; i++ {
		out = append(out, g.script(filters[r.Intn(len(filters))]))
	}

	return out
}

func TestResultsStayValidAcrossWritesToOtherKeys(t *testing.T) {
	setup(t, true)
	if err := api_storage.CreateEntityType("step"); err != nil {
		t.Fatal(err)
	}

	for i := 0; i < 60; i++ {
		_ = api_storage.WriteEntity("step", map[string]any{"id": fmt.Sprintf("s%02d", i), "program": fmt.Sprintf("p%d", i%6), "status": "vu"})
	}

	script := `function postRead(ctx) {
  ctx.entity.ids = ctx.query("step", { program: { eq: ctx.entity.id } }).map(function (d) { return d.id + ":" + d.status }).join(",")
}`
	run := func(program string) string {
		hook.ResetOutputMemo()
		out := map[string]any{"id": program}
		if err := hook.ApplyPostReadScript("", script, out, true); err != nil {
			t.Fatal(err)
		}
		return out["ids"].(string)
	}

	run("p0")
	base := run("p0")

	before := hook.QueryCache()
	for i := 0; i < 30; i++ {
		id := fmt.Sprintf("s%02d", 1+i%5*6)
		api_storage.UpdateEntityById("step", id, map[string]any{"status": fmt.Sprintf("v%d", i)})
		if got := run("p0"); got != base {
			t.Fatalf("a write to another program changed p0: %s", got)
		}
	}

	after := hook.QueryCache()
	if after.Misses != before.Misses {
		t.Fatalf("writes to other keys must not rebuild the result: %+v -> %+v", before, after)
	}

	api_storage.UpdateEntityById("step", "s00", map[string]any{"status": "changed"})
	if got := run("p0"); !strings.Contains(got, "s00:changed") {
		t.Fatalf("a write to the key must be seen at once: %s", got)
	}

	api_storage.UpdateEntityById("step", "s06", map[string]any{"program": "p1"})
	if got := run("p0"); strings.Contains(got, "s06") {
		t.Fatalf("a document that left the key must disappear: %s", got)
	}
	if got := run("p1"); !strings.Contains(got, "s06") {
		t.Fatalf("a document that joined the key must appear: %s", got)
	}

	api_storage.DeleteEntityById("step", "s12")
	if got := run("p0"); strings.Contains(got, "s12") {
		t.Fatalf("a deleted document must disappear: %s", got)
	}
}
