package api_storage

import (
	"strings"

	"github.com/taymour/elysiandb/internal/storage"
)

func getNestedValue(data map[string]interface{}, path string) (string, bool) {
	parts := strings.Split(path, ".")
	var current interface{} = data
	for _, part := range parts {
		m, ok := current.(map[string]interface{})
		if !ok {
			return "", false
		}
		
		current, ok = m[part]
		if !ok {
			return "", false
		}
	}

	strVal, isString := current.(string)
	if !isString {
		return "", false
	}

	return strVal, true
}

func FiltersMatchEntity(
	entityData map[string]interface{},
	filters map[string]map[string]string,
) bool {
	if len(filters) == 0 {
		return true
	}

	for field, ops := range filters {
		strVal, ok := getNestedValue(entityData, field)
		if !ok {
			return false
		}

		for op, val := range ops {
			switch op {
			case "eq":
				if !storage.MatchGlob(val, strVal) {
					return false
				}
			case "neq":
				if storage.MatchGlob(val, strVal) {
					return false
				}
			}
		}
	}

	return true
}
