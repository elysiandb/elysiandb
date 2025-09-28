package api_storage

import (
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/storage"
)

func GetNestedValue(data map[string]interface{}, path string) (interface{}, bool) {
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
		val, ok := GetNestedValue(entityData, field)
		if !ok {
			return false
		}

		switch v := val.(type) {
		case string:
			if !matchStringOrDate(v, ops) {
				return false
			}
		case float64:
			if !matchNumber(v, ops) {
				return false
			}
		default:
			return false
		}
	}

	return true
}

func matchDate(value string, ops map[string]string) (bool, bool) {
	tVal, ok1, dateOnly1 := parseDate(value)
	if !ok1 {
		return false, false
	}

	for op, cmp := range ops {
		tCmp, ok2, dateOnly2 := parseDate(cmp)
		if !ok2 {
			return false, false
		}

		tv, tc := tVal, tCmp
		if dateOnly1 || dateOnly2 {
			tv = tv.Truncate(24 * time.Hour)
			tc = tc.Truncate(24 * time.Hour)
		}

		switch op {
		case "eq":
			if !tv.Equal(tc) {
				return true, false
			}
		case "neq":
			if tv.Equal(tc) {
				return true, false
			}
		case "lt":
			if !(tv.Before(tc)) {
				return true, false
			}
		case "lte":
			if !(tv.Before(tc) || tv.Equal(tc)) {
				return true, false
			}
		case "gt":
			if !(tv.After(tc)) {
				return true, false
			}
		case "gte":
			if !(tv.After(tc) || tv.Equal(tc)) {
				return true, false
			}
		}
	}
	return true, true
}

func matchString(value string, ops map[string]string) bool {
	for op, cmp := range ops {
		switch op {
		case "eq":
			if !storage.MatchGlob(cmp, value) {
				return false
			}
		case "neq":
			if storage.MatchGlob(cmp, value) {
				return false
			}
		}
	}
	return true
}

func matchStringOrDate(value string, ops map[string]string) bool {
	if handled, ok := matchDate(value, ops); handled {
		return ok
	}

	return matchString(value, ops)
}

func matchNumber(value float64, ops map[string]string) bool {
	for op, cmp := range ops {
		num, err := strconv.ParseFloat(cmp, 64)
		if err != nil {
			return false
		}
		switch op {
		case "eq":
			if value != num {
				return false
			}
		case "neq":
			if value == num {
				return false
			}
		case "lt":
			if !(value < num) {
				return false
			}
		case "lte":
			if !(value <= num) {
				return false
			}
		case "gt":
			if !(value > num) {
				return false
			}
		case "gte":
			if !(value >= num) {
				return false
			}
		}
	}
	return true
}

func parseDate(s string) (time.Time, bool, bool) {
	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t, true, false
	}
	if t, err := time.Parse("2006-01-02", s); err == nil {
		return t, true, true
	}
	return time.Time{}, false, false
}
