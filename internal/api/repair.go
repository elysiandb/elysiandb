package api_storage

import (
	"maps"
	"slices"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

type IndexRepair struct {
	EntityTypes []string
	IDsAdded    int
	IDsRemoved  int
}

func (r IndexRepair) Changed() bool {
	return len(r.EntityTypes) > 0 || r.IDsAdded > 0 || r.IDsRemoved > 0
}

func RepairIndexes() IndexRepair {
	documents := map[string][]string{}
	for _, key := range storage.ListJsonKeys() {
		if entity, id, ok := parseDocumentKey(key); ok {
			documents[entity] = append(documents[entity], id)
		}
	}

	repair := IndexRepair{EntityTypes: []string{}}

	known := map[string]bool{}
	for _, entity := range ListEntityTypes() {
		known[entity] = true
	}

	for _, entity := range slices.Sorted(maps.Keys(documents)) {
		if !known[entity] {
			AddEntityType(entity)
			known[entity] = true
			repair.EntityTypes = append(repair.EntityTypes, entity)
		}
	}

	for _, entity := range slices.Sorted(maps.Keys(known)) {
		added, removed := repairIDIndex(entity, documents[entity])
		repair.IDsAdded += added
		repair.IDsRemoved += removed
	}

	return repair
}

func parseDocumentKey(key string) (string, string, bool) {
	rest, ok := strings.CutPrefix(key, globals.ApiKeyPrefix)
	if !ok {
		return "", "", false
	}

	entity, id, ok := strings.Cut(rest, ":id:")
	if !ok || entity == "" || id == "" {
		return "", "", false
	}

	return entity, id, true
}

func repairIDIndex(entity string, ids []string) (int, int) {
	key := globals.ApiEntityIndexIdKey(entity)
	raw, _ := storage.GetByKey(key)
	current := decodeIDs(raw)

	present := make(map[string]bool, len(ids))
	for _, id := range ids {
		present[id] = true
	}

	next := make([]string, 0, len(ids))
	seen := make(map[string]bool, len(ids))
	removed := 0
	for _, id := range current {
		if !present[id] || seen[id] {
			removed++
			continue
		}

		seen[id] = true
		next = append(next, id)
	}

	missing := make([]string, 0)
	for _, id := range ids {
		if !seen[id] {
			seen[id] = true
			missing = append(missing, id)
		}
	}

	if len(missing) == 0 && removed == 0 {
		return 0, 0
	}

	slices.Sort(missing)
	next = append(next, missing...)

	if len(next) == 0 {
		storage.DeleteByKey(key)
	} else {
		_ = storage.PutKeyValue(key, encodeIDs(next))
	}

	for _, field := range GetListForIndexedFields(entity) {
		MarkFieldDirty(entity, field)
	}

	return len(missing), removed
}
