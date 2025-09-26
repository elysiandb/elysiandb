package api_storage

import (
	"strconv"
	"strings"

	"github.com/taymour/elysiandb/internal/storage"
)

func getNestedValue(data map[string]interface{}, path string) (interface{}, bool) {
	parts := strings.Split(path, ".")
	var current interface{} = data
	for _, part := range parts {
		m, ok := current.(map[string]interface{})
		if !ok {
			return nil, false
		}

		current, ok = m[part]
		if !ok {
			return nil, false
		}
	}
	
	return current, true
}

func FiltersMatchEntity(
	entityData map[string]interface{},
	filters map[string]map[string]string,
) bool {
	if len(filters) == 0 {
		return true
	}

	for field, ops := range filters {
		val, ok := getNestedValue(entityData, field)
		if !ok {
			return false
		}

		switch v := val.(type) {
		case string:
			for op, cmp := range ops {
				switch op {
				case "eq":
					if !storage.MatchGlob(cmp, v) {
						return false
					}
				case "neq":
					if storage.MatchGlob(cmp, v) {
						return false
					}
				}
			}
		case float64:
			for op, cmp := range ops {
				num, err := strconv.ParseFloat(cmp, 64)
				if err != nil {
					return false
				}
				switch op {
				case "eq":
					if v != num {
						return false
					}
				case "neq":
					if v == num {
						return false
					}
				case "lt":
					if !(v < num) {
						return false
					}
				case "lte":
					if !(v <= num) {
						return false
					}
				case "gt":
					if !(v > num) {
						return false
					}
				case "gte":
					if !(v >= num) {
						return false
					}
				}
			}
		default:
			return false
		}
	}

	return true
}
