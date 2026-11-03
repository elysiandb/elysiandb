package api_storage

import (
	"strings"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func init() {
	storage.ApiKeyResolver = resolveApiKey
}

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
