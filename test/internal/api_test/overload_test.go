package api_test

import (
	"errors"
	"fmt"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

const overloadDocs = 5000

func initOverloadStore(t *testing.T, query configuration.ApiQueryConfig) {
	t.Helper()

	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{Folder: t.TempDir(), Shards: 8},
		Api:   configuration.ApiConfig{Query: query},
	})
	storage.LoadDB()
	storage.LoadJsonDB()
	t.Cleanup(func() { _ = storage.Close() })

	list := make([]map[string]any, 0, overloadDocs)
	for i := 0; i < overloadDocs; i++ {
		list = append(list, map[string]any{"id": fmt.Sprintf("d%05d", i), "note": strings.Repeat("n", 20), "k": i % 7})
	}
	api_storage.WriteListOfEntities("big", list)
}

func heavyQuery(guard *api_storage.Guard) api_storage.ListQuery {
	return api_storage.ListQuery{
		Entity:    "big",
		Filters:   map[string]map[string]string{"note": {"neq": "x"}},
		Search:    "nnn",
		CountOnly: true,
		Guard:     guard,
	}
}

type laneHolder struct {
	inside  chan struct{}
	release chan struct{}
	done    chan *api_storage.ListResult
}

func holdHeavyLane(t *testing.T) *laneHolder {
	t.Helper()

	h := &laneHolder{inside: make(chan struct{}), release: make(chan struct{}), done: make(chan *api_storage.ListResult, 1)}
	var calls atomic.Int64
	guard := &api_storage.Guard{Abandoned: func() bool {
		if calls.Add(1) < 3 {
			time.Sleep(3 * time.Millisecond)
			return false
		}
		if calls.Load() == 3 {
			close(h.inside)
			<-h.release
		}
		return false
	}}

	go func() { h.done <- api_storage.Plan(heavyQuery(guard)) }()

	select {
	case <-h.inside:
	case <-time.After(5 * time.Second):
		t.Fatalf("the holder never entered the heavy lane")
	}

	t.Cleanup(h.free)

	return h
}

func (h *laneHolder) free() {
	select {
	case <-h.release:
	default:
		close(h.release)
	}
}

func slowGuard(gone *atomic.Bool) *api_storage.Guard {
	return &api_storage.Guard{Abandoned: func() bool {
		time.Sleep(time.Millisecond)
		return gone != nil && gone.Load()
	}}
}

func waitForQueue(t *testing.T, n int) {
	t.Helper()

	deadline := time.Now().Add(5 * time.Second)
	for api_storage.HeavyWaiting() != n {
		if time.Now().After(deadline) {
			t.Fatalf("expected %d queued heavy queries, got %d", n, api_storage.HeavyWaiting())
		}
		time.Sleep(time.Millisecond)
	}
}

func TestHeavyLaneRejectsRequestsWhenItsQueueIsFull(t *testing.T) {
	initOverloadStore(t, configuration.ApiQueryConfig{Workers: 1, MaxQueued: 1, QueueTimeoutMs: -1})
	holder := holdHeavyLane(t)

	queued := make(chan *api_storage.ListResult, 1)
	go func() { queued <- api_storage.Plan(heavyQuery(slowGuard(nil))) }()
	waitForQueue(t, 1)

	guard := slowGuard(nil)
	start := time.Now()
	res := api_storage.Plan(heavyQuery(guard))
	if !errors.Is(res.Err, api_storage.ErrBusy) || !errors.Is(guard.Err(), api_storage.ErrBusy) {
		t.Fatalf("a request beyond the queue limit is rejected, got %v / %v", res.Err, guard.Err())
	}
	if time.Since(start) > time.Second {
		t.Fatalf("the rejection comes after a few milliseconds of work, took %s", time.Since(start))
	}

	internal := make(chan *api_storage.ListResult, 1)
	go func() { internal <- api_storage.Plan(heavyQuery(slowGuard(nil).Until(time.Now().Add(time.Minute)))) }()
	waitForQueue(t, 2)

	holder.free()
	for name, ch := range map[string]chan *api_storage.ListResult{"holder": holder.done, "queued": queued, "internal": internal} {
		select {
		case r := <-ch:
			if r.Err != nil || r.Total != overloadDocs {
				t.Fatalf("%s: expected %d documents, got %d (%v)", name, overloadDocs, r.Total, r.Err)
			}
		case <-time.After(10 * time.Second):
			t.Fatalf("%s never finished", name)
		}
	}
}

func TestQueuedHeavyQueryLeavesWhenItsClientIsGone(t *testing.T) {
	initOverloadStore(t, configuration.ApiQueryConfig{Workers: 1, QueueTimeoutMs: -1})
	holder := holdHeavyLane(t)

	var gone atomic.Bool
	result := make(chan *api_storage.ListResult, 1)
	go func() {
		result <- api_storage.Plan(heavyQuery(slowGuard(&gone)))
	}()
	waitForQueue(t, 1)

	gone.Store(true)
	select {
	case r := <-result:
		if !errors.Is(r.Err, api_storage.ErrAbandoned) {
			t.Fatalf("expected the abandoned error, got %v", r.Err)
		}
	case <-time.After(2 * time.Second):
		t.Fatalf("an abandoned request should leave the queue")
	}

	if api_storage.HeavyWaiting() != 0 {
		t.Fatalf("the queue should be empty, got %d", api_storage.HeavyWaiting())
	}

	holder.free()
	<-holder.done
}

func TestQueuedHeavyQueryGivesUpAfterTheQueueTimeout(t *testing.T) {
	initOverloadStore(t, configuration.ApiQueryConfig{Workers: 1, QueueTimeoutMs: 100})
	holder := holdHeavyLane(t)

	start := time.Now()
	res := api_storage.Plan(heavyQuery(slowGuard(nil)))
	waited := time.Since(start)
	if !errors.Is(res.Err, api_storage.ErrBusy) {
		t.Fatalf("expected busy after the queue timeout, got %v", res.Err)
	}
	if waited < 100*time.Millisecond || waited > 2*time.Second {
		t.Fatalf("the request should wait about the queue timeout, waited %s", waited)
	}

	admitted := make(chan *api_storage.ListResult, 1)
	go func() {
		admitted <- api_storage.Plan(heavyQuery(slowGuard(nil).Until(time.Now().Add(time.Minute))))
	}()
	time.Sleep(300 * time.Millisecond)
	if api_storage.HeavyWaiting() != 1 {
		t.Fatalf("an admitted query waits past the queue timeout")
	}

	holder.free()
	if r := <-admitted; r.Err != nil || r.Total != overloadDocs {
		t.Fatalf("the admitted query should finish, got %d %v", r.Total, r.Err)
	}
	<-holder.done
}

func TestAbandonStopsAScanAndKeepsItsError(t *testing.T) {
	initOverloadStore(t, configuration.ApiQueryConfig{QueueTimeoutMs: -1})

	var checks atomic.Int64
	guard := &api_storage.Guard{Abandoned: func() bool { return checks.Add(1) > 3 }}
	res := api_storage.Plan(heavyQuery(guard))
	if !errors.Is(res.Err, api_storage.ErrAbandoned) || res.Total != 0 || res.Docs != nil {
		t.Fatalf("an abandoned scan returns nothing, got %d %v", res.Total, res.Err)
	}
	if n := checks.Load(); n > 5 {
		t.Fatalf("the scan should stop at the first failed check, checked %d times", n)
	}

	again := api_storage.Plan(api_storage.ListQuery{Entity: "big", Limit: 1, Guard: guard})
	if !errors.Is(again.Err, api_storage.ErrAbandoned) {
		t.Fatalf("a stopped guard stops every later plan of the request, got %v", again.Err)
	}

	if !errors.Is(guard.Until(time.Now().Add(time.Hour)).Check(), api_storage.ErrAbandoned) {
		t.Fatalf("a child guard sees the error of its parent")
	}

	child := (&api_storage.Guard{}).Until(time.Now().Add(-time.Millisecond))
	if !errors.Is(child.Check(), api_storage.ErrDeadline) {
		t.Fatalf("a child guard stops at its own deadline")
	}

	var nilGuard *api_storage.Guard
	if nilGuard.Check() != nil || nilGuard.Err() != nil || nilGuard.Stopped() {
		t.Fatalf("a nil guard never stops")
	}
}

func TestACheapPlanNeverWaitsForTheHeavyLane(t *testing.T) {
	initOverloadStore(t, configuration.ApiQueryConfig{Workers: 1, MaxQueued: 1, QueueTimeoutMs: -1})
	holder := holdHeavyLane(t)

	start := time.Now()
	res := api_storage.Plan(api_storage.ListQuery{Entity: "big", Search: "nnn", Limit: 10, Guard: &api_storage.Guard{}})
	if res.Err != nil || len(res.Docs) != 10 {
		t.Fatalf("a search page is served while the lane is held, got %d %v", len(res.Docs), res.Err)
	}
	if time.Since(start) > time.Second || api_storage.HeavyWaiting() != 0 {
		t.Fatalf("a search page does not queue, took %s with %d waiting", time.Since(start), api_storage.HeavyWaiting())
	}

	holder.free()
	<-holder.done
}
