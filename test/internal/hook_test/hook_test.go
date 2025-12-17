package hook_test

import (
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/storage"
)

func setup(t *testing.T, hooksEnabled bool) {
	t.Helper()

	cfg := &configuration.Config{
		Store: configuration.StoreConfig{
			Folder: t.TempDir(),
			Shards: 2,
			CrashRecovery: configuration.CrashRecoveryConfig{
				Enabled:  false,
				MaxLogMB: 1,
			},
		},
		Security: configuration.SecurityConfig{
			Authentication: configuration.AuthenticationConfig{
				Enabled: false,
				Mode:    "basic",
				Token:   "",
			},
		},
		Stats: configuration.StatsConfig{
			Enabled: false,
		},
		Api: configuration.ApiConfig{
			Schema: configuration.ApiSchemaConfig{
				Enabled: false,
				Strict:  false,
			},
			Cache: configuration.ApiCacheConfig{
				Enabled:                false,
				CleanupIntervalSeconds: 0,
			},
			Index: configuration.ApiIndexConfig{
				Workers: 0,
			},
			Hooks: configuration.HooksConfig{
				Enabled: hooksEnabled,
			},
		},
		AdminUI: configuration.AdminUIConfig{
			Enabled: false,
		},
	}

	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()
}

func TestGetDefaultHookScriptJSForPostRead(t *testing.T) {
	setup(t, true)

	s := hook.GetDefaultHookScriptJSForPostRead()
	if s == "" {
		t.Fatalf("expected non-empty default script")
	}
	if len(s) < 10 {
		t.Fatalf("expected default script to be longer")
	}
}

func TestGetDefaultHookScriptJSForPreRead(t *testing.T) {
	setup(t, true)

	s := hook.GetDefaultHookScriptJSForPreRead()
	if s == "" {
		t.Fatalf("expected non-empty default script")
	}
	if len(s) < 10 {
		t.Fatalf("expected default script to be longer")
	}
}

func TestHookToDataMapAndFromDataMap(t *testing.T) {
	setup(t, true)

	h := hook.Hook{
		ID:        "id1",
		Name:      "name",
		Script:    "script",
		Entity:    "toto",
		Event:     hook.HookEventPostRead,
		Language:  "javascript",
		Priority:  7,
		ByPassACL: true,
		Enabled:   true,
	}

	m := h.ToDataMap()

	var out hook.Hook
	out.FromDataMap(m)

	if out.ID != h.ID || out.Name != h.Name || out.Script != h.Script || out.Entity != h.Entity || out.Event != h.Event || out.Language != h.Language || out.Priority != h.Priority || out.ByPassACL != h.ByPassACL || out.Enabled != h.Enabled {
		t.Fatalf("roundtrip mismatch: %+v != %+v", out, h)
	}

	m["priority"] = float64(12)
	var out2 hook.Hook
	out2.FromDataMap(m)
	if out2.Priority != 12 {
		t.Fatalf("expected priority 12, got %d", out2.Priority)
	}

	m["priority"] = int64(9)
	var out3 hook.Hook
	out3.FromDataMap(m)
	if out3.Priority != 9 {
		t.Fatalf("expected priority 9, got %d", out3.Priority)
	}
}

func TestInitHooksDisabledDoesNothing(t *testing.T) {
	setup(t, false)

	hook.InitHooks()
	if api_storage.EntityTypeExists(hook.HookEntity) {
		t.Fatalf("did not expect hook entity type to exist when hooks disabled")
	}
}

func TestInitHooksCreatesEntityType(t *testing.T) {
	setup(t, true)

	if api_storage.EntityTypeExists(hook.HookEntity) {
		t.Fatalf("expected hook entity type to not exist yet")
	}

	hook.InitHooks()

	if !api_storage.EntityTypeExists(hook.HookEntity) {
		t.Fatalf("expected hook entity type to exist after init")
	}
}

func TestCreateHookEntity(t *testing.T) {
	setup(t, true)

	if api_storage.EntityTypeExists(hook.HookEntity) {
		t.Fatalf("expected hook entity type to not exist yet")
	}

	if err := hook.CreateHookEntity(); err != nil {
		t.Fatalf("expected no error, got %v", err)
	}

	if !api_storage.EntityTypeExists(hook.HookEntity) {
		t.Fatalf("expected hook entity type to exist")
	}
}

func TestApplyPostReadScriptMissingPostReadIsNoop(t *testing.T) {
	setup(t, true)

	entity := map[string]any{"id": "1", "x": 0}
	script := `function other(ctx){ return ctx.entity }`
	if err := hook.ApplyPostReadScript(script, entity, true); err != nil {
		t.Fatalf("expected no error, got %v", err)
	}
	if entity["x"].(int) != 0 {
		t.Fatalf("expected x unchanged")
	}
}

func TestApplyPostReadScriptSyntaxError(t *testing.T) {
	setup(t, true)

	entity := map[string]any{"id": "1"}
	script := `function postRead(ctx) {`
	if err := hook.ApplyPostReadScript(script, entity, true); err == nil {
		t.Fatalf("expected error")
	}
}

func TestApplyPostReadScriptQueryArgError(t *testing.T) {
	setup(t, true)

	entity := map[string]any{"id": "1"}
	script := `
function postRead(ctx) {
  ctx.query("order")
  return ctx.entity
}`
	if err := hook.ApplyPostReadScript(script, entity, true); err == nil {
		t.Fatalf("expected error")
	}
}

func TestApplyPostReadScriptMutatesEntityAndQueryWorks(t *testing.T) {
	setup(t, true)

	if err := api_storage.CreateEntityType("order"); err != nil {
		t.Fatalf("create order entity type: %v", err)
	}

	_ = api_storage.WriteEntity("order", map[string]any{"id": "o1", "totoId": "1"})
	_ = api_storage.WriteEntity("order", map[string]any{"id": "o2", "totoId": "1"})
	_ = api_storage.WriteEntity("order", map[string]any{"id": "o3", "totoId": "2"})

	entity := map[string]any{"id": "1"}

	script := `
function postRead(ctx) {
  const orders = ctx.query("order", { totoId: { eq: ctx.entity.id } })
  ctx.entity.ordersCount = orders.length
  ctx.entity.ok = true
  return ctx.entity
}`
	if err := hook.ApplyPostReadScript(script, entity, true); err != nil {
		t.Fatalf("expected no error, got %v", err)
	}

	if entity["ordersCount"] != int64(2) && entity["ordersCount"] != 2 {
		t.Fatalf("expected ordersCount to be 2, got %#v", entity["ordersCount"])
	}
	if b, ok := entity["ok"].(bool); !ok || !b {
		t.Fatalf("expected ok=true, got %#v", entity["ok"])
	}
}

func TestCreateUpdateDeleteGetHookAndQueries(t *testing.T) {
	setup(t, true)

	hook.InitHooks()

	h := hook.Hook{
		Entity:    "toto",
		Name:      "h1",
		Event:     hook.HookEventPostRead,
		Language:  "javascript",
		Priority:  1,
		Enabled:   true,
		ByPassACL: false,
		Script:    "",
	}

	if err := hook.CreateHook(h); err != nil {
		t.Fatalf("create hook: %v", err)
	}

	if !hook.EntityHasHooks("toto") {
		t.Fatalf("expected EntityHasHooks to be true")
	}

	list, err := hook.GetHooksForEntity("toto")
	if err != nil {
		t.Fatalf("GetHooksForEntity: %v", err)
	}
	if len(list) != 1 {
		t.Fatalf("expected 1 hook, got %d", len(list))
	}
	id := list[0].ID
	if id == "" {
		t.Fatalf("expected generated id")
	}
	if list[0].Script == "" {
		t.Fatalf("expected default script to be set")
	}

	got, err := hook.GetHookById(id)
	if err != nil {
		t.Fatalf("GetHookById: %v", err)
	}
	if got.ID != id {
		t.Fatalf("expected id %s, got %s", id, got.ID)
	}

	got.Name = "h1b"
	got.Priority = 3
	got.Enabled = false

	if err := hook.UpdateHook(got); err == nil {
		t.Fatalf("expected UpdateHook to error (id is not updatable in current storage layer)")
	}

	if err := hook.DeleteHook(id); err != nil {
		t.Fatalf("DeleteHook: %v", err)
	}

	if hook.EntityHasHooks("toto") {
		t.Fatalf("expected EntityHasHooks to be false after delete")
	}

	if _, err := hook.GetHookById(id); err == nil {
		t.Fatalf("expected error for missing hook")
	}

	if err := hook.DeleteHook(id); err == nil {
		t.Fatalf("expected error when deleting missing hook")
	}

	missing := &hook.Hook{ID: "missing", Name: "missing"}
	if err := hook.UpdateHook(missing); err == nil {
		t.Fatalf("expected error when updating missing hook")
	}
}

func TestCreateHookDuplicateID(t *testing.T) {
	setup(t, true)

	hook.InitHooks()

	id := "same-id"

	h1 := hook.Hook{
		ID:       id,
		Entity:   "toto",
		Name:     "h1",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Script:   "function postRead(ctx){ return ctx.entity }",
		Enabled:  true,
		Priority: 1,
	}

	if err := hook.CreateHook(h1); err != nil {
		t.Fatalf("create hook: %v", err)
	}

	h2 := hook.Hook{
		ID:       id,
		Entity:   "toto",
		Name:     "h2",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Script:   "function postRead(ctx){ return ctx.entity }",
		Enabled:  true,
		Priority: 2,
	}

	if err := hook.CreateHook(h2); err == nil {
		t.Fatalf("expected error for duplicate id")
	}
}

func TestGetPostReadHooksForEntityFiltersEvent(t *testing.T) {
	setup(t, true)

	hook.InitHooks()

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "a",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Script:   "function postRead(ctx){ ctx.entity.a = 1; return ctx.entity }",
		Enabled:  true,
		Priority: 1,
	})

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "b",
		Event:    "other_event",
		Language: "javascript",
		Script:   "function postRead(ctx){ ctx.entity.b = 1; return ctx.entity }",
		Enabled:  true,
		Priority: 1,
	})

	list := hook.GetPostReadHooksForEntity("toto")
	if len(list) != 1 {
		t.Fatalf("expected 1 post_read hook, got %d", len(list))
	}
	if list[0].Event != hook.HookEventPostRead {
		t.Fatalf("expected event post_read, got %s", list[0].Event)
	}
}

func TestApplyPostReadHooksForEntityPriorityEnabledAndErrorTolerance(t *testing.T) {
	setup(t, true)

	hook.InitHooks()

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "high",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Script:   "function postRead(ctx){ ctx.entity.steps = (ctx.entity.steps || '') + 'A'; return ctx.entity }",
		Enabled:  true,
		Priority: 10,
	})

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "low",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Script:   "function postRead(ctx){ ctx.entity.steps = (ctx.entity.steps || '') + 'B'; return ctx.entity }",
		Enabled:  true,
		Priority: 1,
	})

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "disabled",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Script:   "function postRead(ctx){ ctx.entity.steps = (ctx.entity.steps || '') + 'X'; return ctx.entity }",
		Enabled:  false,
		Priority: 100,
	})

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "broken",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Script:   "function postRead(ctx){ throw new Error('boom') }",
		Enabled:  true,
		Priority: 5,
	})

	entity := map[string]any{"id": "1"}

	out := hook.ApplyPostReadHooksForEntity("toto", entity)
	if out == nil {
		t.Fatalf("expected non-nil entity")
	}

	steps, _ := out["steps"].(string)
	if steps != "AB" && steps != "A"+"B" {
		t.Fatalf("expected steps to be AB, got %#v", out["steps"])
	}
}

func TestGetPreReadHooksForEntityFiltersEvent(t *testing.T) {
	setup(t, true)

	hook.InitHooks()

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "a",
		Event:    hook.HookEventPreRead,
		Language: "javascript",
		Script:   "function preRead(ctx){ return ctx.entity }",
		Enabled:  true,
		Priority: 1,
	})

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "b",
		Event:    hook.HookEventPostRead,
		Language: "javascript",
		Script:   "function postRead(ctx){ return ctx.entity }",
		Enabled:  true,
		Priority: 1,
	})

	list := hook.GetPreReadHooksForEntity("toto")
	if len(list) != 1 {
		t.Fatalf("expected 1 pre_read hook, got %d", len(list))
	}
	if list[0].Event != hook.HookEventPreRead {
		t.Fatalf("expected event pre_read, got %s", list[0].Event)
	}
}

func TestApplyPreReadHooksForEntityPriorityEnabledAndErrorTolerance(t *testing.T) {
	setup(t, true)

	hook.InitHooks()

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "high",
		Event:    hook.HookEventPreRead,
		Language: "javascript",
		Script:   "function preRead(ctx){ ctx.entity.steps = (ctx.entity.steps || '') + 'A'; return ctx.entity }",
		Enabled:  true,
		Priority: 10,
	})

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "low",
		Event:    hook.HookEventPreRead,
		Language: "javascript",
		Script:   "function preRead(ctx){ ctx.entity.steps = (ctx.entity.steps || '') + 'B'; return ctx.entity }",
		Enabled:  true,
		Priority: 1,
	})

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "disabled",
		Event:    hook.HookEventPreRead,
		Language: "javascript",
		Script:   "function preRead(ctx){ ctx.entity.steps = (ctx.entity.steps || '') + 'X'; return ctx.entity }",
		Enabled:  false,
		Priority: 100,
	})

	_ = hook.CreateHook(hook.Hook{
		Entity:   "toto",
		Name:     "broken",
		Event:    hook.HookEventPreRead,
		Language: "javascript",
		Script:   "function preRead(ctx){ throw new Error('boom') }",
		Enabled:  true,
		Priority: 5,
	})

	entity := map[string]any{"id": "1"}

	out := hook.ApplyPreReadHooksForEntity("toto", entity)
	if out == nil {
		t.Fatalf("expected non-nil entity")
	}

	steps, _ := out["steps"].(string)
	if steps != "AB" && steps != "A"+"B" {
		t.Fatalf("expected steps to be AB, got %#v", out["steps"])
	}
}

func TestApplyPreReadScriptMissingPreReadIsNoop(t *testing.T) {
	setup(t, true)

	entity := map[string]any{"id": "1", "x": 0}
	script := `function other(ctx){ return ctx.entity }`
	if err := hook.ApplyPreReadScript(script, entity, true); err != nil {
		t.Fatalf("expected no error, got %v", err)
	}
	if entity["x"].(int) != 0 {
		t.Fatalf("expected x unchanged")
	}
}

func TestApplyPreReadScriptSyntaxError(t *testing.T) {
	setup(t, true)

	entity := map[string]any{"id": "1"}
	script := `function preRead(ctx) {`
	if err := hook.ApplyPreReadScript(script, entity, true); err == nil {
		t.Fatalf("expected error")
	}
}

func TestApplyPreReadScriptQueryArgError(t *testing.T) {
	setup(t, true)

	entity := map[string]any{"id": "1"}
	script := `
function preRead(ctx) {
  ctx.query("order")
  return ctx.entity
}`
	if err := hook.ApplyPreReadScript(script, entity, true); err == nil {
		t.Fatalf("expected error")
	}
}

func TestApplyPreReadScriptMutatesEntityAndQueryWorks(t *testing.T) {
	setup(t, true)

	if err := api_storage.CreateEntityType("order"); err != nil {
		t.Fatalf("create order entity type: %v", err)
	}

	_ = api_storage.WriteEntity("order", map[string]any{"id": "o1", "totoId": "1"})
	_ = api_storage.WriteEntity("order", map[string]any{"id": "o2", "totoId": "1"})
	_ = api_storage.WriteEntity("order", map[string]any{"id": "o3", "totoId": "2"})

	entity := map[string]any{"id": "1"}

	script := `
function preRead(ctx) {
  const orders = ctx.query("order", { totoId: { eq: ctx.entity.id } })
  ctx.entity.ordersCount = orders.length
  ctx.entity.ok = true
  return ctx.entity
}`
	if err := hook.ApplyPreReadScript(script, entity, true); err != nil {
		t.Fatalf("expected no error, got %v", err)
	}

	if entity["ordersCount"] != int64(2) && entity["ordersCount"] != 2 {
		t.Fatalf("expected ordersCount to be 2, got %#v", entity["ordersCount"])
	}
	if b, ok := entity["ok"].(bool); !ok || !b {
		t.Fatalf("expected ok=true, got %#v", entity["ok"])
	}
}
