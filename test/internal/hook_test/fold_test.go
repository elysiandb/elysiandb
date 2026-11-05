package hook_test

import (
	"fmt"
	"math/rand"
	"sync"
	"sync/atomic"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/hook"
)

const budgetScript = `function monthsBetween(from, to) {
  var a = from.split("-"), b = to.split("-")
  return (parseInt(b[0], 10) - parseInt(a[0], 10)) * 12 + parseInt(b[1], 10) - parseInt(a[1], 10)
}
function postRead(ctx) {
  var month = ctx.entity.id
  var purchases = ctx.query("purchase", { status: { neq: "annule" } })
  var total = 0, count = 0
  for (var i = 0; i < purchases.length; i++) {
    var p = purchases[i]
    if (!p.budget_month || p.status === "wishlist") { continue }
    var n = p.installments || 1
    if (n > 1) {
      var k = monthsBetween(p.budget_month, month)
      if (k < 0 || k >= n) { continue }
      total += p.installment_amount || 0
    } else {
      if (p.budget_month !== month) { continue }
      total += p.price || 0
    }
    count++
  }
  total += ctx.entity.spent_manual_adjust || 0
  ctx.entity.spent = Math.round(total * 100) / 100
  ctx.entity.remaining = Math.round(((ctx.entity.cap || 0) - total) * 100) / 100
  ctx.entity.purchase_count = count
  ctx.entity.raw_total = total
}`

const progressScript = `function postRead(ctx) {
  var steps = ctx.query("step", { program_id: { eq: ctx.entity.id } })
  var done = 0, total = 0
  for (var i = 0; i < steps.length; i++) {
    if (steps[i].status === "vu") { done++ }
    if (steps[i].status !== "hors_marathon") { total++ }
  }
  ctx.entity.steps_done = done
  ctx.entity.steps_total = total
  ctx.entity.progress_label = done + "/" + total
}`

func monthID(i int) string {
	return fmt.Sprintf("20%02d-%02d", 20+i/12, 1+i%12)
}

func randomPurchase(r *rand.Rand, id string) map[string]any {
	doc := map[string]any{"id": id, "budget_month": monthID(r.Intn(36))}
	switch r.Intn(6) {
	case 0:
		doc["status"] = "annule"
	case 1:
		doc["status"] = "wishlist"
	default:
		doc["status"] = "achete"
	}

	doc["price"] = float64(r.Intn(100000)) / 100
	if r.Intn(4) == 0 {
		doc["installments"] = float64(2 + r.Intn(10))
		doc["installment_amount"] = float64(r.Intn(10000)) / 100
	}

	switch r.Intn(40) {
	case 0:
		delete(doc, "price")
	case 1:
		doc["budget_month"] = ""
	case 2:
		delete(doc, "status")
	case 3:
		doc["budget_month"] = nil
	}

	return doc
}

func runBudget(t *testing.T, month string) map[string]any {
	t.Helper()
	out := map[string]any{"id": month, "cap": 500.0, "spent_manual_adjust": 1.5}
	if err := hook.ApplyPostReadScript("", budgetScript, out, true); err != nil {
		t.Fatal(err)
	}

	return out
}

func runProgress(t *testing.T, program string) map[string]any {
	t.Helper()
	out := map[string]any{"id": program}
	if err := hook.ApplyPostReadScript("", progressScript, out, true); err != nil {
		t.Fatal(err)
	}

	return out
}

func sameDocs(a, b map[string]any) bool {
	if len(a) != len(b) {
		return false
	}

	for k, v := range a {
		if fmt.Sprint(b[k]) != fmt.Sprint(v) {
			return false
		}
	}

	return true
}

func compareBudget(t *testing.T, months int, label string) {
	t.Helper()
	for i := 0; i < months; i++ {
		month := monthID(i)

		hook.ResetOutputMemo()
		hook.SetFoldsEnabled(true)
		fast := runBudget(t, month)

		hook.ResetOutputMemo()
		hook.SetFoldsEnabled(false)
		slow := runBudget(t, month)
		hook.SetFoldsEnabled(true)

		if !sameDocs(fast, slow) {
			hook.ResetQueryCache()
			hook.ResetOutputMemo()
			fresh := runBudget(t, month)
			t.Fatalf("%s month %s: fold %v, script %v, fresh %v, %+v", label, month, fast, slow, fresh, hook.Folds())
		}
	}
}

func TestBudgetFoldAnswersLikeTheScript(t *testing.T) {
	setup(t, true)
	if err := api_storage.CreateEntityType("purchase"); err != nil {
		t.Fatal(err)
	}

	r := rand.New(rand.NewSource(7))
	for i := 0; i < 1200; i++ {
		_ = api_storage.WriteEntity("purchase", randomPurchase(r, fmt.Sprintf("p%05d", i)))
	}

	before := hook.Folds()
	compareBudget(t, 36, "initial")
	if got := hook.Folds(); got.FullPasses == before.FullPasses {
		t.Fatalf("the budget script should run on the view, stats %+v", got)
	}

	for round := 0; round < 40; round++ {
		id := fmt.Sprintf("p%05d", r.Intn(1200))
		switch r.Intn(4) {
		case 0:
			api_storage.DeleteEntityById("purchase", id)
		case 1:
			_ = api_storage.WriteEntity("purchase", randomPurchase(r, fmt.Sprintf("n%05d", round)))
		default:
			api_storage.UpdateEntityById("purchase", id, randomPurchase(r, id))
		}

		compareBudget(t, 6, fmt.Sprintf("round %d", round))
	}

	stats := hook.Folds()
	if stats.Incremental == 0 {
		t.Fatalf("writes should advance the kept sums, stats %+v", stats)
	}
}

func TestProgressFoldAnswersLikeTheScript(t *testing.T) {
	setup(t, true)
	if err := api_storage.CreateEntityType("step"); err != nil {
		t.Fatal(err)
	}

	statuses := []string{"vu", "a_voir", "hors_marathon"}
	r := rand.New(rand.NewSource(11))
	for i := 0; i < 600; i++ {
		_ = api_storage.WriteEntity("step", map[string]any{"id": fmt.Sprintf("s%05d", i), "program_id": fmt.Sprintf("Prog-%d", r.Intn(30)), "status": statuses[r.Intn(3)]})
	}

	check := func(label string) {
		t.Helper()
		for i := 0; i < 31; i++ {
			program := fmt.Sprintf("prog-%d", i)

			hook.ResetOutputMemo()
			hook.SetFoldsEnabled(true)
			fast := runProgress(t, program)

			hook.ResetOutputMemo()
			hook.SetFoldsEnabled(false)
			slow := runProgress(t, program)
			hook.SetFoldsEnabled(true)

			if !sameDocs(fast, slow) {
				t.Fatalf("%s %s: fold %v, script %v", label, program, fast, slow)
			}
		}
	}

	check("initial")
	for round := 0; round < 30; round++ {
		id := fmt.Sprintf("s%05d", r.Intn(600))
		if round%5 == 4 {
			api_storage.DeleteEntityById("step", id)
		} else {
			api_storage.UpdateEntityById("step", id, map[string]any{"program_id": fmt.Sprintf("prog-%d", r.Intn(30)), "status": statuses[r.Intn(3)]})
		}
		check(fmt.Sprintf("round %d", round))
	}

	if fb := hook.Folds(); fb.Views == 0 {
		t.Fatalf("the progress script should run on a view, stats %+v", fb)
	}
}

func TestFoldFallsBackOnValuesItDoesNotHandle(t *testing.T) {
	setup(t, true)
	if err := api_storage.CreateEntityType("step"); err != nil {
		t.Fatal(err)
	}

	docs := []map[string]any{
		{"id": "a", "program_id": "p", "status": "vu"},
		{"id": "b", "program_id": "p", "status": map[string]any{"nested": true}},
		{"id": "c", "program_id": 5.0, "status": "vu"},
		{"id": "d", "program_id": []any{"p", "q"}, "status": "vu"},
		{"id": "e", "status": "vu"},
	}
	for _, d := range docs {
		_ = api_storage.WriteEntity("step", d)
	}

	for _, program := range []string{"p", "5", "p,q", "P", "*", "2026-01-01", ""} {
		hook.ResetOutputMemo()
		hook.SetFoldsEnabled(true)
		fast := runProgress(t, program)

		hook.ResetOutputMemo()
		hook.SetFoldsEnabled(false)
		slow := runProgress(t, program)
		hook.SetFoldsEnabled(true)

		if !sameDocs(fast, slow) {
			t.Fatalf("%q: fold %v, script %v", program, fast, slow)
		}
	}
}

func TestKeptResultsDoNotSurviveTheirEngine(t *testing.T) {
	script := `function postRead(ctx) {
  ctx.entity.ids = ctx.query("twice", {}).map(function (d) { return d.id }).join(",")
}`
	run := func() string {
		hook.ResetOutputMemo()
		out := map[string]any{"id": "x"}
		if err := hook.ApplyPostReadScript("", script, out, true); err != nil {
			t.Fatal(err)
		}
		return out["ids"].(string)
	}

	setup(t, true)
	_ = api_storage.CreateEntityType("twice")
	_ = api_storage.WriteEntity("twice", map[string]any{"id": "old"})
	if got := run(); got != "old" {
		t.Fatalf("first engine sees %q", got)
	}

	setup(t, true)
	_ = api_storage.CreateEntityType("twice")
	_ = api_storage.WriteEntity("twice", map[string]any{"id": "a"})
	_ = api_storage.WriteEntity("twice", map[string]any{"id": "b"})
	if got := run(); got != "a,b" {
		t.Fatalf("second engine sees %q", got)
	}
}

func TestFoldStaysExactUnderConcurrentWrites(t *testing.T) {
	setup(t, true)
	if err := api_storage.CreateEntityType("purchase"); err != nil {
		t.Fatal(err)
	}

	r := rand.New(rand.NewSource(3))
	for i := 0; i < 800; i++ {
		_ = api_storage.WriteEntity("purchase", randomPurchase(r, fmt.Sprintf("p%05d", i)))
	}

	var stop atomic.Bool
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		w := rand.New(rand.NewSource(5))
		for i := 0; !stop.Load(); i++ {
			id := fmt.Sprintf("p%05d", w.Intn(800))
			api_storage.UpdateEntityById("purchase", id, randomPurchase(w, id))
		}
	}()

	for i := 0; i < 200; i++ {
		hook.ResetOutputMemo()
		out := runBudget(t, monthID(i%36))
		if _, ok := out["spent"]; !ok {
			t.Fatalf("no result %v", out)
		}
	}

	stop.Store(true)
	wg.Wait()

	compareBudget(t, 36, "after the writers")
}
