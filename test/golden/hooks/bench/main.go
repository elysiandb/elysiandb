package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"math/rand"
	"os"
	"path/filepath"
	"runtime"
	"sort"
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

const plainArraysScript = `
function postRead(ctx) {
  var a = ctx.query("order", { group: { eq: "g0" } })
  ctx.entity.isArray = Array.isArray(a)
  ctx.entity.len = a.length
  ctx.entity.typeofRow = typeof a[0]
  ctx.entity.keys = Object.keys(a[0]).sort().join(",")
  ctx.entity.hasOwn = a[0].hasOwnProperty("amount") && !a[0].hasOwnProperty("missing")
  ctx.entity.inOp = ("group" in a[0]) && !("nope" in a[0])
  ctx.entity.missing = a[0].nope === undefined
  a[0].amount = 1000
  a[0].extra = "x"
  delete a[0].group
  ctx.entity.mutated = a[0].amount + ":" + a[0].extra + ":" + (a[0].group === undefined)
  ctx.entity.mutKeys = Object.keys(a[0]).sort().join(",")
  a[0].group = "back"
  ctx.entity.readded = Object.keys(a[0]).filter(function (k) { return k === "group" }).length
  a[1].tags.push("pushed")
  a[1].owner.name = "changed"
  ctx.entity.nested = a[1].tags.length + ":" + a[1].owner.name
  var b = ctx.query("order", { group: { eq: "g0" } })
  ctx.entity.isolated = b[0].amount + ":" + b[0].group + ":" + b[1].tags.length + ":" + b[1].owner.name + ":" + (b[0].extra === undefined)
  a.push({ id: "new" })
  ctx.entity.pushed = a.length + ":" + a[a.length - 1].id + ":" + b.length
  var sorted = b.slice().sort(function (x, y) { return y.amount - x.amount })
  ctx.entity.sorted = sorted.map(function (o) { return o.id }).join(",")
  b.length = 1
  ctx.entity.truncated = b.length + ":" + (b[1] === undefined)
  b.length = 2
  ctx.entity.regrown = b.length + ":" + (b[1] === undefined)
  ctx.entity.json = JSON.stringify(ctx.query("order", { id: { eq: "o3" } }))
  var total = 0
  ctx.query("order", {}).forEach(function (o) { total += o.amount })
  ctx.entity.total = total
  var forin = []
  for (var k in ctx.query("order", { id: { eq: "o0" } })[0]) { forin.push(k) }
  ctx.entity.forin = forin.sort().join(",")
}`

const cpuScript = `function postRead(ctx) {
  var parts = []
  for (var i = 0; i < 100000; i++) { parts.push("item-" + ((i * 7919) % 100000)) }
  var s = parts.join(",")
  var nums = []
  for (var j = 0; j < 100000; j++) { nums.push((j * 104729) % 100003) }
  nums.sort(function (a, b) { return a - b })
  ctx.entity.len = s.length
  ctx.entity.first = nums[0]
  ctx.entity.last = nums[nums.length - 1]
}`

const input = `{"id":"x","price":12,"ratio":0.5,"tags":["a","b"],"meta":{"k":"v","n":1},"nul":null,"s":"str","b":true,"big":12345678901234567890}`

type result struct {
	Name   string  `json:"name"`
	Runs   int     `json:"runs"`
	NsP50  int64   `json:"ns_p50"`
	NsP99  int64   `json:"ns_p99"`
	Allocs float64 `json:"allocs"`
	Error  string  `json:"error,omitempty"`
}

var tmpRoot string
var storeN int

func openStore() {
	storeN++
	cfg := &configuration.Config{}
	cfg.Store.Folder = filepath.Join(tmpRoot, fmt.Sprintf("s%03d", storeN))
	cfg.Store.Shards = 4
	cfg.Api.Hooks.Enabled = true
	cfg.Log.Level = "error"
	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()
	cache.InitCache(30 * time.Second)
	hook.InitHooks()
}

func measure(name, script string, entity func() map[string]any, minRuns int, minTime time.Duration) result {
	for i := 0; i < 3; i++ {
		_ = hook.ApplyPostReadScript("", script, entity(), true)
	}
	var times []int64
	var ms0, ms1 runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&ms0)
	start := time.Now()
	var lastErr error
	for len(times) < minRuns || time.Since(start) < minTime {
		e := entity()
		t0 := time.Now()
		lastErr = hook.ApplyPostReadScript("", script, e, true)
		times = append(times, time.Since(t0).Nanoseconds())
		if len(times) >= 100000 {
			break
		}
	}
	runtime.ReadMemStats(&ms1)
	sort.Slice(times, func(i, j int) bool { return times[i] < times[j] })
	r := result{Name: name, Runs: len(times), NsP50: times[len(times)/2], NsP99: times[len(times)*99/100], Allocs: float64(ms1.Mallocs-ms0.Mallocs) / float64(len(times))}
	if lastErr != nil {
		r.Error = "error"
	}
	return r
}

func main() {
	out := flag.String("out", "", "tests/golden/hooks")
	casesPath := flag.String("cases", "test/golden/hooks/cases.jsonl", "corpus case table")
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "missing -out")
		os.Exit(2)
	}
	dir, err := os.MkdirTemp("", "elysiandb-golden-hook-bench-")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	tmpRoot = dir
	defer os.RemoveAll(dir)
	log.SetOutput(io.Discard, false)
	var results []result
	plain := func() map[string]any { return map[string]any{"id": "x"} }

	openStore()
	results = append(results, measure("trivial", `function postRead(ctx) { ctx.entity.x = 1 }`, plain, 2000, 2*time.Second))
	_ = storage.Close()

	openStore()
	_ = api_storage.CreateEntityType("purchase")
	r := rand.New(rand.NewSource(7))
	for i := 0; i < 1200; i++ {
		_ = api_storage.WriteEntity("purchase", randomPurchase(r, fmt.Sprintf("p%05d", i)))
	}
	budget := func() map[string]any {
		return map[string]any{"id": "2021-06", "cap": 500.0, "spent_manual_adjust": 1.5}
	}
	hook.SetFoldsEnabled(true)
	results = append(results, measure("budget-folds-on", budgetScript, budget, 500, 2*time.Second))
	hook.SetFoldsEnabled(false)
	results = append(results, measure("budget-folds-off", budgetScript, budget, 200, 2*time.Second))
	hook.SetFoldsEnabled(true)
	_ = storage.Close()

	openStore()
	_ = api_storage.CreateEntityType("step")
	statuses := []string{"vu", "a_voir", "hors_marathon"}
	r = rand.New(rand.NewSource(11))
	for i := 0; i < 600; i++ {
		_ = api_storage.WriteEntity("step", map[string]any{"id": fmt.Sprintf("s%05d", i), "program_id": fmt.Sprintf("Prog-%d", r.Intn(30)), "status": statuses[r.Intn(3)]})
	}
	progress := func() map[string]any { return map[string]any{"id": "Prog-3"} }
	hook.SetFoldsEnabled(true)
	results = append(results, measure("progress-folds-on", progressScript, progress, 500, 2*time.Second))
	hook.SetFoldsEnabled(false)
	results = append(results, measure("progress-folds-off", progressScript, progress, 200, 2*time.Second))
	hook.SetFoldsEnabled(true)
	_ = storage.Close()

	openStore()
	_ = api_storage.CreateEntityType("order")
	for i := 0; i < 6; i++ {
		_ = api_storage.WriteEntity("order", map[string]any{"id": fmt.Sprintf("o%d", i), "group": fmt.Sprintf("g%d", i%3), "amount": float64(i), "tags": []any{"a", fmt.Sprintf("t%d", i)}, "owner": map[string]any{"name": "n", "level": float64(i % 2)}})
	}
	results = append(results, measure("plain-arrays", plainArraysScript, plain, 500, 2*time.Second))
	_ = storage.Close()

	openStore()
	results = append(results, measure("cpu-bound", cpuScript, plain, 20, 3*time.Second))
	_ = storage.Close()

	f, err := os.Open(*casesPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 1<<20), 1<<24)
	openStore()
	for i := 0; i < 3; i++ {
		_ = api_storage.WriteEntity("order", map[string]any{"id": fmt.Sprintf("o%d", i), "group": "g", "amount": float64(i) + 0.5, "tags": []any{"t"}, "owner": map[string]any{"n": "x"}})
	}
	corpusEntity := func() map[string]any {
		var e map[string]any
		_ = json.Unmarshal([]byte(input), &e)
		return e
	}
	for sc.Scan() {
		var c struct {
			Label  string `json:"label"`
			Script string `json:"script"`
		}
		if err := json.Unmarshal(sc.Bytes(), &c); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if c.Label == "timeout" || c.Label == "timeout-catch" || c.Label == "timeout-recursion" || c.Label == "deep-recursion" {
			continue
		}
		results = append(results, measure("corpus/"+c.Label, c.Script, corpusEntity, 50, 200*time.Millisecond))
	}
	f.Close()
	_ = storage.Close()

	if err := goldenio.WriteJSON(*out, "bench-baseline.json", map[string]any{
		"go_version": runtime.Version(), "goos": runtime.GOOS, "goarch": runtime.GOARCH, "cpus": runtime.NumCPU(), "baseline": goldenio.Baseline, "cases": results,
	}); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

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
