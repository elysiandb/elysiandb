package api_storage

import (
	"strconv"
	"strings"

	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func init() {
	storage.ApiKeyResolver = resolveApiKey
	cache.VersionOf = entityVersion
}

func entityVersion(entity string) string {
	e := db()
	if e == nil {
		return ""
	}

	snap := e.Snapshot(entity)

	return strconv.FormatUint(snap.Generation(), 36) + "." + strconv.FormatUint(snap.Version(), 36)
}

func CacheDependencies(entity, includes string, withACL bool) []string {
	deps := []string{entity}
	if withACL {
		deps = append(deps, aclEntity)
	}

	if includes == "" {
		return deps
	}

	e := db()
	if e == nil {
		return deps
	}

	seen := map[string]bool{entity: true}
	queue := []string{entity}
	for len(queue) > 0 {
		current := queue[0]
		queue = queue[1:]
		for _, ref := range e.Snapshot(current).Refs() {
			if !seen[ref.Target] && !IsCoreEntityType(ref.Target) {
				seen[ref.Target] = true
				deps = append(deps, ref.Target)
				queue = append(queue, ref.Target)
			}
		}
	}

	return deps
}

const aclEntity = "_elysiandb_core_acl"

func sortSpec(field string) datastore.IndexSpec {
	return datastore.IndexSpec{Kind: datastore.IndexSort, Field: field}
}

func IndexExistsForField(entity, field string) bool {
	e := db()
	if e == nil {
		return false
	}

	e.EnsureIndex(entity, sortSpec(field))

	return e.Snapshot(entity).HasIndex(sortSpec(field))
}

func EnsureSortIndex(entity, field string) bool {
	e := db()
	if e == nil {
		return false
	}

	return e.EnsureIndex(entity, sortSpec(field))
}

func GetListForIndexedFields(entity string) []string {
	e := db()
	if e == nil {
		return nil
	}

	fields := []string{}
	for _, name := range e.IndexNames(entity) {
		if spec, ok := datastore.ParseIndexName(name); ok && spec.Kind == datastore.IndexSort {
			fields = append(fields, spec.Field)
		}
	}

	return fields
}

func GetSortedEntityIdsByField(entity, field string, ascending bool) []string {
	return Plan(ListQuery{Entity: entity, SortField: field, SortAsc: ascending}).IDs()
}

func IndexBacklog() int {
	e := db()
	if e == nil {
		return 0
	}

	return e.IndexBacklog()
}

func resolveApiKey(e *datastore.Engine, key string) ([]byte, bool) {
	if key == globals.ApiAllEntityTypesListKey() {
		types := e.Types()
		if len(types) == 0 {
			return nil, false
		}

		return []byte(strings.Join(types, "\n")), true
	}

	rest, ok := strings.CutPrefix(key, globals.ApiKeyPrefix)
	if !ok {
		return nil, false
	}

	entity, index, ok := strings.Cut(rest, ":internal:index:")
	if !ok || entity == "" || !e.TypeExists(entity) {
		return nil, false
	}

	switch {
	case index == "id":
		ids := Plan(ListQuery{Entity: entity}).IDs()
		if len(ids) == 0 {
			return nil, false
		}

		return []byte(strings.Join(ids, "\n")), true
	case index == "fields:all":
		fields := GetListForIndexedFields(entity)
		if len(fields) == 0 {
			return nil, false
		}

		return []byte(strings.Join(fields, "\n")), true
	case strings.HasPrefix(index, "field:") && (strings.HasSuffix(index, ":sort:asc") || strings.HasSuffix(index, ":sort:desc")):
		ascending := strings.HasSuffix(index, ":sort:asc")
		field := strings.TrimSuffix(strings.TrimSuffix(strings.TrimPrefix(index, "field:"), ":sort:asc"), ":sort:desc")
		if !e.Snapshot(entity).HasIndex(sortSpec(field)) {
			return nil, false
		}

		return []byte(strings.Join(GetSortedEntityIdsByField(entity, field, ascending), "\n")), true
	}

	return nil, false
}
