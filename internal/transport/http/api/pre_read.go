package api

import (
	"github.com/taymour/elysiandb/internal/datastore"
	"runtime"
	"sync"
	"sync/atomic"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/hook"
)

var (
	hookPoolOnce sync.Once
	hookPool     chan struct{}
)

func hookHelpers() chan struct{} {
	hookPoolOnce.Do(func() {
		hookPool = make(chan struct{}, max(1, runtime.GOMAXPROCS(0)))
	})

	return hookPool
}

func applyPreReadHooksAndFilter(
	hooks *hook.Request,
	entity string,
	data []map[string]any,
	matches func(map[string]any) bool,
	offset int,
	limit int,
) []map[string]any {
	selected := make([]map[string]any, 0)
	for _, item := range data {
		if hooks.Stopped() {
			break
		}

		item = hooks.PreRead(entity, item)
		if !matches(item) {
			continue
		}

		selected = append(selected, item)
		if limit > 0 && len(selected) >= offset+limit {
			break
		}
	}

	return api_storage.Paginate(selected, offset, limit)
}

func applyPostReadHooks(hooks *hook.Request, entity string, data []map[string]any) {
	var next atomic.Int64
	run := func() {
		for {
			i := int(next.Add(1)) - 1
			if i >= len(data) || hooks.Stopped() {
				return
			}
			data[i] = hooks.PostRead(entity, data[i])
		}
	}

	if len(data) <= 1 {
		run()
		return
	}

	pool := hookHelpers()
	want := min(len(data), cap(pool)) - 1
	var wg sync.WaitGroup
spawn:
	for h := 0; h < want; h++ {
		select {
		case pool <- struct{}{}:
		default:
			break spawn
		}

		wg.Add(1)
		go func() {
			defer wg.Done()
			defer func() { <-pool }()
			run()
		}()
	}

	run()
	wg.Wait()
}

func lazyPreReadPage(
	hooks *hook.Request,
	entity string,
	each func(func(datastore.Doc) bool),
	includes string,
	stored func(map[string]any) bool,
	candidates api_storage.SeqFilter,
	mode acl.ReadMode,
	username string,
	matches func(map[string]any) bool,
	offset int,
	limit int,
) []map[string]any {
	selected := make([]map[string]any, 0)
	each(func(d datastore.Doc) bool {
		if candidates != nil && !candidates(d.Seq()) {
			return true
		}

		if hooks.Stopped() {
			return false
		}

		item, err := d.Decode()
		if err != nil {
			return true
		}

		if includes != "" {
			item = api_storage.ApplyIncludes([]map[string]any{item}, includes)[0]
		}

		if !stored(item) {
			return true
		}

		if mode == acl.ReadNone || (mode == acl.ReadOwned && !acl.OwnedBy(item, username)) {
			return true
		}

		item = hooks.PreRead(entity, item)
		if !matches(item) {
			return true
		}

		selected = append(selected, item)

		return limit <= 0 || len(selected) < offset+limit
	})

	return api_storage.Paginate(selected, offset, limit)
}
