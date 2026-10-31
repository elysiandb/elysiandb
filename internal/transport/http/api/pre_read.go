package api

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/hook"
)

func applyPreReadHooksAndFilter(
	username string,
	entity string,
	data []map[string]any,
	matches func(map[string]any) bool,
	offset int,
	limit int,
) []map[string]any {
	selected := make([]map[string]any, 0)
	for _, item := range data {
		item = hook.ApplyPreReadHooksForEntity(username, entity, item)
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
