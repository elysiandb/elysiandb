package api_test

import (
	"errors"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/memory"
)

func TestHeavyRequestsAreRefusedOverTheMemoryBudget(t *testing.T) {
	initOverloadStore(t, configuration.ApiQueryConfig{Workers: 1, QueueTimeoutMs: 100})
	t.Cleanup(func() { memory.SetBudget(0) })

	memory.SetBudget(1)

	guard := slowGuard(nil)
	res := api_storage.Plan(heavyQuery(guard))
	if !errors.Is(res.Err, api_storage.ErrBusy) || !errors.Is(guard.Err(), api_storage.ErrBusy) {
		t.Fatalf("a heavy request over the budget is answered busy, got %v / %v", res.Err, guard.Err())
	}

	patient := slowGuard(nil).Until(time.Now().Add(time.Minute))
	if res := api_storage.Plan(heavyQuery(patient)); !errors.Is(res.Err, api_storage.ErrBusy) {
		t.Fatalf("a hook query over the budget stops too, got %v", res.Err)
	}

	listGuard := &api_storage.Guard{}
	docs, err := api_storage.ListEntitiesGuarded("big", 0, 0, "", true, nil, "", "", listGuard)
	if !errors.Is(err, api_storage.ErrBusy) || len(docs) != 0 {
		t.Fatalf("decoding a large list over the budget stops, got %d documents and %v", len(docs), err)
	}

	if res := api_storage.Plan(heavyQuery(nil)); res.Err != nil || res.Total != overloadDocs {
		t.Fatalf("internal reads without a guard are never refused, got %d %v", res.Total, res.Err)
	}

	memory.SetBudget(0)
	if res := api_storage.Plan(heavyQuery(slowGuard(nil))); res.Err != nil || res.Total != overloadDocs {
		t.Fatalf("under the budget the request runs, got %d %v", res.Total, res.Err)
	}

	docs, err = api_storage.ListEntitiesGuarded("big", 0, 0, "", true, nil, "", "", &api_storage.Guard{})
	if err != nil || len(docs) != overloadDocs {
		t.Fatalf("under the budget the list is decoded, got %d %v", len(docs), err)
	}
}
