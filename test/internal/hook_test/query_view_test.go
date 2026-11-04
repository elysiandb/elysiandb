package hook_test

import (
	"encoding/json"
	"fmt"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/hook"
)

func seedOrders(t *testing.T, n int) {
	t.Helper()

	if err := api_storage.CreateEntityType("order"); err != nil {
		t.Fatalf("create order entity type: %v", err)
	}

	for i := 0; i < n; i++ {
		_ = api_storage.WriteEntity("order", map[string]any{
			"id":     fmt.Sprintf("o%d", i),
			"group":  fmt.Sprintf("g%d", i%3),
			"amount": float64(i),
			"tags":   []any{"a", fmt.Sprintf("t%d", i)},
			"owner":  map[string]any{"name": "n", "level": float64(i % 2)},
		})
	}
}

func runPostRead(t *testing.T, script string, entity map[string]any) map[string]any {
	t.Helper()

	if err := hook.ApplyPostReadScript("", script, entity, true); err != nil {
		t.Fatalf("hook failed: %v", err)
	}

	return entity
}

func TestQueryResultsBehaveLikePlainArrays(t *testing.T) {
	setup(t, true)
	seedOrders(t, 6)

	script := `
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

	got := runPostRead(t, script, map[string]any{"id": "x"})

	want := map[string]any{
		"isArray":   true,
		"len":       int64(2),
		"typeofRow": "object",
		"keys":      "amount,group,id,owner,tags",
		"hasOwn":    true,
		"inOp":      true,
		"missing":   true,
		"mutated":   "1000:x:true",
		"mutKeys":   "amount,extra,id,owner,tags",
		"readded":   int64(1),
		"nested":    "3:changed",
		"isolated":  "0:g0:2:n:true",
		"pushed":    "3:new:2",
		"sorted":    "o3,o0",
		"truncated": "1:true",
		"regrown":   "2:true",
		"total":     int64(15),
		"forin":     "amount,group,id,owner,tags",
	}

	for k, v := range want {
		if fmt.Sprint(got[k]) != fmt.Sprint(v) {
			t.Errorf("%s = %#v, want %#v", k, got[k], v)
		}
	}

	var decoded []map[string]any
	if err := json.Unmarshal([]byte(got["json"].(string)), &decoded); err != nil || len(decoded) != 1 || decoded[0]["id"] != "o3" || decoded[0]["amount"] != float64(3) {
		t.Fatalf("JSON.stringify of a query result = %v (%v)", got["json"], err)
	}
}

func TestQueryResultsFollowWrites(t *testing.T) {
	setup(t, true)
	seedOrders(t, 4)

	script := `
function postRead(ctx) {
  var total = 0
  var rows = ctx.query("order", { group: { neq: "none" } })
  for (var i = 0; i < rows.length; i++) { total += rows[i].amount }
  ctx.entity.total = total
  ctx.entity.count = rows.length
}`

	first := runPostRead(t, script, map[string]any{"id": "x"})
	if fmt.Sprint(first["total"]) != "6" || fmt.Sprint(first["count"]) != "4" {
		t.Fatalf("first run = %v", first)
	}

	_ = api_storage.WriteEntity("order", map[string]any{"id": "o9", "group": "g1", "amount": float64(10)})
	second := runPostRead(t, script, map[string]any{"id": "x"})
	if fmt.Sprint(second["total"]) != "16" || fmt.Sprint(second["count"]) != "5" {
		t.Fatalf("after a write = %v", second)
	}

	_ = api_storage.UpdateEntityById("order", "o0", map[string]any{"amount": float64(100)})
	third := runPostRead(t, script, map[string]any{"id": "x"})
	if fmt.Sprint(third["total"]) != "116" {
		t.Fatalf("after an update = %v", third)
	}

	api_storage.DeleteEntityById("order", "o9")
	fourth := runPostRead(t, script, map[string]any{"id": "x"})
	if fmt.Sprint(fourth["total"]) != "106" || fmt.Sprint(fourth["count"]) != "4" {
		t.Fatalf("after a delete = %v", fourth)
	}
}

func TestDeterministicOutputsAreReusedUntilReadEntitiesChange(t *testing.T) {
	setup(t, true)
	seedOrders(t, 3)
	hook.ResetOutputMemo()

	script := `
function postRead(ctx) {
  var rows = ctx.query("order", { group: { eq: ctx.entity.group } })
  ctx.entity.sum = rows.reduce(function (s, o) { return s + o.amount }, 0)
  delete ctx.entity.drop
}`

	before := hook.OutputMemo()
	a := runPostRead(t, script, map[string]any{"id": "x", "group": "g1", "drop": true})
	b := runPostRead(t, script, map[string]any{"id": "x", "group": "g1", "drop": true})
	after := hook.OutputMemo()
	if after.Hits-before.Hits != 1 {
		t.Fatalf("expected one reused output, stats %+v → %+v", before, after)
	}

	if fmt.Sprint(a["sum"]) != "1" || fmt.Sprint(b["sum"]) != "1" {
		t.Fatalf("sums %v %v", a, b)
	}

	if _, ok := b["drop"]; ok {
		t.Fatalf("a reused output must also delete fields: %v", b)
	}

	_ = api_storage.WriteEntity("order", map[string]any{"id": "o7", "group": "g1", "amount": float64(5)})
	c := runPostRead(t, script, map[string]any{"id": "x", "group": "g1", "drop": true})
	if fmt.Sprint(c["sum"]) != "6" {
		t.Fatalf("an output must be recomputed after a write: %v", c)
	}

	d := runPostRead(t, script, map[string]any{"id": "x", "group": "g2", "drop": true})
	if fmt.Sprint(d["sum"]) != "2" {
		t.Fatalf("another input gets its own output: %v", d)
	}
}

func TestNonDeterministicScriptsAreNeverReused(t *testing.T) {
	setup(t, true)
	seedOrders(t, 1)
	hook.ResetOutputMemo()

	for _, script := range []string{
		`function postRead(ctx) { ctx.entity.v = Date.now() }`,
		`function postRead(ctx) { ctx.entity.v = new Date().getTime() }`,
		`function postRead(ctx) { ctx.entity.v = Date() }`,
		`function postRead(ctx) { ctx.entity.v = Math.random() }`,
		`function postRead(ctx) { ctx.entity.v = new (new Date(0).constructor)().getTime() }`,
		`function postRead(ctx) { var D = this["Da" + "te"]; ctx.entity.v = D.now() }`,
		`function postRead(ctx) { var M = this["Ma" + "th"]; ctx.entity.v = M["ran" + "dom"]() }`,
	} {
		before := hook.OutputMemo()
		runPostRead(t, script, map[string]any{"id": "x"})
		runPostRead(t, script, map[string]any{"id": "x"})
		if after := hook.OutputMemo(); after.Hits != before.Hits {
			t.Fatalf("%s: a time or random dependent output was reused", script)
		}
	}

	fixed := `function postRead(ctx) { ctx.entity.v = new Date(86400000).toISOString(); ctx.entity.u = Date.UTC(2020, 0, 1); ctx.entity.ok = new Date(0) instanceof Date }`
	before := hook.OutputMemo()
	first := runPostRead(t, fixed, map[string]any{"id": "x"})
	second := runPostRead(t, fixed, map[string]any{"id": "x"})
	if after := hook.OutputMemo(); after.Hits-before.Hits != 1 {
		t.Fatalf("a script using only fixed dates should be reused")
	}

	for _, e := range []map[string]any{first, second} {
		if e["v"] != "1970-01-02T00:00:00.000Z" || fmt.Sprint(e["u"]) != "1577836800000" || e["ok"] != true {
			t.Fatalf("fixed dates: %v", e)
		}
	}
}

func TestFailingScriptsAreNotReused(t *testing.T) {
	setup(t, true)
	seedOrders(t, 1)
	hook.ResetOutputMemo()

	script := `function postRead(ctx) { ctx.entity.partial = 1; throw new Error("boom") }`
	before := hook.OutputMemo()
	for i := 0; i < 2; i++ {
		entity := map[string]any{"id": "x"}
		if err := hook.ApplyPostReadScript("", script, entity, true); err == nil {
			t.Fatalf("expected the script error")
		}
		if fmt.Sprint(entity["partial"]) != "1" {
			t.Fatalf("partial mutations stay visible as before: %v", entity)
		}
	}

	if after := hook.OutputMemo(); after.Hits != before.Hits {
		t.Fatalf("a failing script must run every time")
	}
}
