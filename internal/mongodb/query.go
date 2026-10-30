package mongodb

import (
	"strings"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/query"
)

func ExecuteQuery(q query.Query) ([]map[string]any, error) {
	sortField := ""
	sortAscending := true

	for field, dir := range q.Sorts {
		sortField = field
		if strings.ToLower(dir) == "desc" {
			sortAscending = false
		}
		break
	}

	wanted := 0
	if q.Limit > 0 {
		wanted = max(q.Offset, 0) + q.Limit
	}

	matches := make([]map[string]any, 0)
	scanEntities(scanRequest{
		entity:        q.Entity,
		filter:        BuildQueryPrefilter(q.Filter),
		sortField:     sortField,
		sortAscending: sortAscending,
		batchSize:     wanted,
	}, func(batch []map[string]any) bool {
		matches = append(matches, api_storage.ApplyQueryFilter(batch, q.Filter)...)

		return wanted == 0 || len(matches) < wanted
	})

	return applyOffsetLimit(matches, q.Offset, q.Limit), nil
}
