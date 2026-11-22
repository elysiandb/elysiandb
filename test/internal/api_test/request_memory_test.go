package api_test

import (
	"errors"
	"fmt"
	"strings"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/memory"
)

func TestRequestsBeyondTheMemoryBudgetAreRejected(t *testing.T) {
	initOverloadStore(t, configuration.ApiQueryConfig{QueueTimeoutMs: 20})
	wide := make([]map[string]any, 0, 2000)
	for i := 0; i < 2000; i++ {
		wide = append(wide, map[string]any{"id": fmt.Sprintf("w%05d", i), "body": strings.Repeat("w", 1024)})
	}
	api_storage.WriteListOfEntities("wide", wide)

	memory.Configure(8 << 20)
	t.Cleanup(func() { memory.Configure(0) })

	guard := &api_storage.Guard{}
	res := api_storage.Plan(api_storage.ListQuery{Entity: "wide", Guard: guard})
	docs := res.Decode()

	if !errors.Is(guard.Err(), api_storage.ErrBusy) || len(docs) != 0 {
		t.Fatalf("a request needing more than the request share must be refused, got %d documents and %v", len(docs), guard.Err())
	}

	guard.ReleaseMemory()
	if used := memory.Requests.Used(); used != 0 {
		t.Fatalf("a finished request gives its memory back, %d bytes still reserved", used)
	}

	small := &api_storage.Guard{}
	page := api_storage.Plan(api_storage.ListQuery{Entity: "wide", Limit: 20, Guard: small})
	if got := page.Decode(); len(got) != 20 || small.Err() != nil {
		t.Fatalf("a small page fits in the budget, got %d documents and %v", len(got), small.Err())
	}
	small.ReleaseMemory()
}

func TestRequestMemoryIsFreeWithoutABudget(t *testing.T) {
	initOverloadStore(t, configuration.ApiQueryConfig{})
	memory.Configure(0)

	guard := &api_storage.Guard{}
	docs := api_storage.Plan(api_storage.ListQuery{Entity: "big", Guard: guard}).Decode()
	if len(docs) != overloadDocs || guard.Err() != nil {
		t.Fatalf("without an explicit budget nothing is refused, got %d documents and %v", len(docs), guard.Err())
	}
}
