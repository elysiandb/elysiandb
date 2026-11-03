package api

import (
	"runtime"
	"sync"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/hook"
)

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
	workers := min(runtime.GOMAXPROCS(0), len(data))
	if workers <= 1 {
		for i, item := range data {
			data[i] = hooks.PostRead(entity, item)
		}
		return
	}

	next := make(chan int)
	var wg sync.WaitGroup
	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := range next {
				data[i] = hooks.PostRead(entity, data[i])
			}
		}()
	}

	for i := range data {
		next <- i
	}
	close(next)
	wg.Wait()
}

func lazyPreReadPage(
	hooks *hook.Request,
	entity string,
	res *api_storage.ListResult,
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
	for _, d := range res.Docs {
		if candidates != nil && !candidates(d.Seq()) {
			continue
		}

		item, err := d.Decode()
		if err != nil {
			continue
		}

		if includes != "" {
			item = api_storage.ApplyIncludes([]map[string]any{item}, includes)[0]
		}

		if !stored(item) {
			continue
		}

		if mode == acl.ReadNone || (mode == acl.ReadOwned && !acl.OwnedBy(item, username)) {
			continue
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
