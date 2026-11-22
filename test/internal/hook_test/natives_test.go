package hook_test

import (
	"fmt"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/hook"
)

func TestNativeAggregatesMatchTheirJavaScriptEquivalent(t *testing.T) {
	setup(t, true)
	if err := api_storage.CreateEntityType("buy"); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 300; i++ {
		doc := map[string]any{"id": fmt.Sprintf("b%03d", i), "month": fmt.Sprintf("2026-%02d", 1+i%12), "price": float64(i % 17), "status": []string{"ok", "annule"}[i%5/4], "meta": map[string]any{"weight": float64(i % 3)}}
		if i%7 == 0 {
			delete(doc, "price")
		}
		_ = api_storage.WriteEntity("buy", doc)
	}

	script := `function postRead(ctx) {
  var f = { status: { neq: "annule" } }
  var rows = ctx.query("buy", f)
  var sum = 0, weight = 0, groups = {}
  for (var i = 0; i < rows.length; i++) {
    var p = rows[i]
    if (typeof p.price === "number") { sum += p.price }
    weight += p.meta.weight
    var g = groups[p.month] || (groups[p.month] = { count: 0, sum: 0 })
    g.count++
    if (typeof p.price === "number") { g.sum += p.price }
  }
  var native = ctx.group("buy", f, "month", "price")
  var same = Object.keys(native).length === Object.keys(groups).length
  Object.keys(groups).forEach(function (k) {
    same = same && native[k].count === groups[k].count && native[k].sum === groups[k].sum
  })
  ctx.entity.count = ctx.count("buy", f) === rows.length
  ctx.entity.sum = ctx.sum("buy", f, "price") === sum
  ctx.entity.nested = ctx.sum("buy", f, "meta.weight") === weight
  ctx.entity.groups = same
  ctx.entity.counts = JSON.stringify(ctx.group("buy", f, "status"))
  ctx.entity.missing = ctx.group("buy", f, "month")["1999-01"] === undefined
}`

	out := map[string]any{"id": "x"}
	if err := hook.ApplyPostReadScript("", script, out, true); err != nil {
		t.Fatal(err)
	}

	for _, k := range []string{"count", "sum", "nested", "groups", "missing"} {
		if out[k] != true {
			t.Fatalf("%s differs from the JavaScript computation: %v", k, out)
		}
	}

	if out["counts"] != `{"ok":{"count":240}}` {
		t.Fatalf("group counts %v", out["counts"])
	}
}
