package hook_test

import (
	"fmt"
	"math/rand"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/storage"
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

func TestResultsFollowTheEntitiesReachedByTheirFilters(t *testing.T) {
	setup(t, true)
	_ = api_storage.CreateEntityType("author")
	_ = api_storage.CreateEntityType("book")
	_ = api_storage.WriteEntity("author", map[string]any{"id": "a1", "name": "first"})
	_ = api_storage.WriteEntity("book", map[string]any{"id": "b1", "author": map[string]any{"@entity": "author", "id": "a1"}})

	script := `function postRead(ctx) {
  ctx.entity.n = ctx.query("book", { "author.name": { eq: "first" } }).length
}`
	run := func() any {
		hook.ResetOutputMemo()
		out := map[string]any{"id": "x"}
		if err := hook.ApplyPostReadScript("", script, out, true); err != nil {
			t.Fatal(err)
		}
		return out["n"]
	}

	if got := fmt.Sprint(run()); got != "1" {
		t.Fatalf("the book is found through its author, got %s", got)
	}

	api_storage.UpdateEntityById("author", "a1", map[string]any{"name": "second"})
	if got := fmt.Sprint(run()); got != "0" {
		t.Fatalf("a change of the author must be seen at once, got %s", got)
	}
}

func TestEqualityKeysCoverEveryKindOfValue(t *testing.T) {
	setup(t, true)
	_ = api_storage.CreateEntityType("mixed")

	values := []any{"a", "A", "1", "true", "x", 1.0, 0.0, true, false, nil, []any{"a"}, []any{"x", "y"}, []any{"1"}, "2026-01-01", map[string]any{"a": 1.0}}
	filters := []string{"a", "A", "1", "0", "true", "false", "x", "x,y", "y,x", "2026-01-01", ""}
	r := rand.New(rand.NewSource(9))

	ids := []string{}
	for i := 0; i < 80; i++ {
		id := fmt.Sprintf("m%03d", i)
		ids = append(ids, id)
		_ = api_storage.WriteEntity("mixed", map[string]any{"id": id, "f": values[r.Intn(len(values))]})
	}

	script := `function postRead(ctx) {
  ctx.entity.ids = ctx.query("mixed", { f: { eq: ctx.entity.want } }).map(function (d) { return d.id }).join(",")
}`
	run := func(f string) string {
		t.Helper()
		hook.ResetOutputMemo()
		out := map[string]any{"id": "x", "want": f}
		if err := hook.ApplyPostReadScript("", script, out, true); err != nil {
			t.Fatal(err)
		}
		return fmt.Sprint(out["ids"])
	}

	check := func() {
		t.Helper()
		for _, f := range filters {
			kept := run(f)

			want := []string{}
			for _, id := range ids {
				d := api_storage.ReadEntityById("mixed", id)
				if d != nil && api_storage.FiltersMatchEntity(d, map[string]map[string]string{"f": {"eq": f}}) {
					want = append(want, id)
				}
			}

			got := strings.Split(kept, ",")
			if kept == "" {
				got = []string{}
			}
			sort.Strings(got)
			if strings.Join(got, ",") != strings.Join(want, ",") {
				t.Fatalf("eq %q: kept result %q, documents %q", f, kept, strings.Join(want, ","))
			}
		}

		for _, f := range filters {
			kept := run(f)
			hook.ResetQueryCache()
			if fresh := run(f); fresh != kept {
				t.Fatalf("eq %q: kept result %q, new result %q", f, kept, fresh)
			}
		}
	}

	check()
	for round := 0; round < 200; round++ {
		id := ids[r.Intn(len(ids))]
		if r.Intn(8) == 0 {
			api_storage.DeleteEntityById("mixed", id)
			_ = api_storage.WriteEntity("mixed", map[string]any{"id": id, "f": values[r.Intn(len(values))]})
		} else {
			api_storage.UpdateEntityById("mixed", id, map[string]any{"f": values[r.Intn(len(values))]})
		}
		if round%10 == 0 {
			check()
		}
	}
	check()
}

func TestViewsFollowWritesBeforeAnyRead(t *testing.T) {
	setup(t, true)
	if err := storage.Open(storage.ModeServer); err != nil {
		t.Fatal(err)
	}
	hook.InitHooks()
	_ = api_storage.CreateEntityType("purchase")

	h := &hook.Hook{Name: "budget", Entity: "month", Event: hook.HookEventPostRead, Language: "javascript", Enabled: true, ByPassACL: true, Script: budgetScript}
	if err := hook.CreateHook(h); err != nil {
		t.Fatal(err)
	}
	hook.PrepareIndexes()

	r := rand.New(rand.NewSource(1))
	for i := 0; i < 2000; i++ {
		_ = api_storage.WriteEntity("purchase", randomPurchase(r, fmt.Sprintf("p%05d", i)))
	}

	deadline := time.Now().Add(10 * time.Second)
	for hook.Folds().Bytes == 0 {
		if time.Now().After(deadline) {
			t.Fatalf("the view should be built without a read, stats %+v", hook.Folds())
		}
		time.Sleep(10 * time.Millisecond)
	}

	time.Sleep(600 * time.Millisecond)

	before := hook.QueryCache()
	out := runBudget(t, monthID(1))
	after := hook.QueryCache()
	if after.Misses != before.Misses || after.Hits == before.Hits {
		t.Fatalf("the first read should find the view ready: %+v -> %+v", before, after)
	}

	hook.SetFoldsEnabled(false)
	defer hook.SetFoldsEnabled(true)
	hook.ResetOutputMemo()
	if slow := runBudget(t, monthID(1)); !sameDocs(out, slow) {
		t.Fatalf("view %v, script %v", out, slow)
	}
}
