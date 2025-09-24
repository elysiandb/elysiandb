package api_storage

import "github.com/taymour/elysiandb/internal/storage"

func FiltersMatchEntity(
	entityData map[string]interface{},
	filters map[string]string,
) bool {
	if len(filters) == 0 {
		return true
	}

	for field, value := range filters {
		entityVal, ok := entityData[field]
		strVal, isString := entityVal.(string)
		if !ok || !isString {
			return false
		}
		if !storage.MatchGlob(value, strVal) {
			return false
		}
	}

	return true
}
