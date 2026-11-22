package hook_test

import (
	"fmt"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/storage"
)

func waitForIndex(t *testing.T, entity, name string) datastore.IndexState {
	t.Helper()

	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		for _, st := range storage.Engine().IndexStates() {
			if st.Entity == entity && st.Index == name && st.State == "ready" {
				return st
			}
		}
		time.Sleep(10 * time.Millisecond)
	}

	t.Fatalf("index %s on %s never became ready, states %+v", name, entity, storage.Engine().IndexStates())

	return datastore.IndexState{}
}

func TestHooksGetTheirQueryIndexesInAdvanceAndKeepThem(t *testing.T) {
	setup(t, true)
	hook.InitHooks()

	if err := api_storage.CreateEntityType("step"); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 50; i++ {
		_ = api_storage.WriteEntity("step", map[string]any{"id": fmt.Sprintf("s%02d", i), "program_id": fmt.Sprintf("p%d", i%5), "status": "vu"})
	}

	h := &hook.Hook{Name: "progress", Entity: "program", Event: hook.HookEventPostRead, Language: "javascript", Enabled: true, ByPassACL: true,
		Script: `function postRead(ctx) {
  var steps = ctx.query("step", { program_id: { eq: ctx.entity.id }, "status": { neq: "x" } })
  ctx.entity.n = steps.length
}`}
	if err := hook.CreateHook(h); err != nil {
		t.Fatal(err)
	}

	for _, name := range []string{"f:program_id", "f:status"} {
		st := waitForIndex(t, "step", name)
		if st.Origin != string(datastore.OriginHook) || !st.Pinned {
			t.Fatalf("%s should be a pinned hook index, got %+v", name, st)
		}
	}

	if dropped := storage.Engine().DropIdleIndexes(0); dropped != 0 {
		for _, st := range storage.Engine().IndexStates() {
			if st.Entity == "step" && st.State != "ready" {
				t.Fatalf("a hook index was released while its hook exists: %+v", st)
			}
		}
	}

	if err := hook.DeleteHook(h.ID); err != nil {
		t.Fatal(err)
	}

	for _, st := range storage.Engine().IndexStates() {
		if st.Entity == "step" && st.Pinned {
			t.Fatalf("deleting the hook should unpin its indexes: %+v", st)
		}
	}
}

func TestDynamicHookQueriesArePreparedAtFirstRun(t *testing.T) {
	setup(t, true)
	hook.InitHooks()

	if err := api_storage.CreateEntityType("rows"); err != nil {
		t.Fatal(err)
	}
	_ = api_storage.WriteEntity("rows", map[string]any{"id": "r1", "group": "g"})

	h := &hook.Hook{Name: "dynamic", Entity: "parent", Event: hook.HookEventPostRead, Language: "javascript", Enabled: true, ByPassACL: true,
		Script: `function postRead(ctx) {
  var field = "gr" + "oup"
  var filter = {}
  filter[field] = { eq: "g" }
  ctx.entity.n = ctx.query("rows", filter).length
}`}
	if err := hook.CreateHook(h); err != nil {
		t.Fatal(err)
	}

	out := hook.ApplyPostReadHooksForEntity("", "parent", map[string]any{"id": "x"})
	if fmt.Sprint(out["n"]) != "1" {
		t.Fatalf("hook result %v", out)
	}

	st := waitForIndex(t, "rows", "f:group")
	if !st.Pinned {
		t.Fatalf("an index seen at the first run should be pinned for the hook: %+v", st)
	}
}
