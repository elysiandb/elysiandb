package api_storage

import (
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func SearchPattern(search string) string {
	if strings.Contains(search, "*") {
		return search
	}

	return "*" + search + "*"
}

func SearchMatchesEntity(entityData map[string]any, search string) bool {
	pattern := SearchPattern(search)

	var stack []any
	stack = append(stack, entityData)

	for len(stack) > 0 {
		n := len(stack) - 1
		cur := stack[n]
		stack = stack[:n]

		switch v := cur.(type) {
		case map[string]any:
			for key, val := range v {
				if !isSearchableKey(key) {
					continue
				}

				stack = append(stack, val)
			}
		case []any:
			stack = append(stack, v...)
		case []map[string]any:
			for _, item := range v {
				stack = append(stack, item)
			}
		case string:
			if storage.MatchGlob(pattern, v) {
				return true
			}
		}
	}

	return false
}

func isSearchableKey(key string) bool {
	return key != "id" && key != "@entity" && !strings.HasPrefix(key, globals.CoreFieldsPrefix)
}

func FiltersMatchEntity(
	entityData map[string]any,
	filters map[string]map[string]string,
) bool {
	return matchLeafStrict(entityData, filters)
}

func StoredFiltersMatchEntity(
	entityData map[string]any,
	filters map[string]map[string]string,
) bool {
	for field, ops := range filters {
		values := resolveValues(entityData, field)
		if len(values) > 0 && !matchAnyValue(values, ops) {
			return false
		}
	}

	return true
}

func matchBoolean(value bool, ops map[string]string) bool {
	for op, cmp := range ops {
		b := cmp == "true" || cmp == "1"
		switch op {
		case "eq":
			if value != b {
				return false
			}
		case "neq":
			if value == b {
				return false
			}
		default:
			return false
		}
	}

	return true
}

func matchArray(arr []any, ops map[string]string) bool {
	toStrings := func(a []any) []string {
		out := make([]string, 0, len(a))
		for _, v := range a {
			switch s := v.(type) {
			case string:
				out = append(out, s)
			case float64:
				out = append(out, strconv.FormatFloat(s, 'f', -1, 64))
			case int:
				out = append(out, strconv.Itoa(s))
			default:
				out = append(out, fmt.Sprintf("%v", s))
			}
		}

		return out
	}

	inArray := func(arr []string, val string) bool {
		for _, a := range arr {
			if a == val {
				return true
			}
		}

		return false
	}

	arrStr := toStrings(arr)
	for op, cmp := range ops {
		values := strings.Split(cmp, ",")

		switch op {
		case "contains":
			if !inArray(arrStr, cmp) {
				return false
			}
		case "not_contains":
			if inArray(arrStr, cmp) {
				return false
			}
		case "all":
			for _, v := range values {
				if !inArray(arrStr, v) {
					return false
				}
			}
		case "any":
			found := false
			for _, v := range values {
				if inArray(arrStr, v) {
					found = true
					break
				}
			}

			if !found {
				return false
			}
		case "eq":
			if len(arrStr) != len(values) {
				return false
			}

			matched := make(map[string]bool)
			for _, v := range arrStr {
				matched[v] = true
			}

			for _, v := range values {
				if !matched[v] {
					return false
				}
			}

		case "none":
			for _, v := range values {
				if inArray(arrStr, v) {
					return false
				}
			}
		default:
			return false
		}
	}

	return true
}

func isComparisonOperator(op string) bool {
	switch op {
	case "eq", "neq", "lt", "lte", "gt", "gte":
		return true
	default:
		return false
	}
}

func matchDate(value string, ops map[string]string) (bool, bool) {
	tVal, ok, dateOnlyVal := parseDate(value)
	if !ok {
		return false, false
	}

	type dateComparison struct {
		op       string
		value    time.Time
		dateOnly bool
	}

	comparisons := make([]dateComparison, 0, len(ops))
	for op, cmp := range ops {
		if !isComparisonOperator(op) {
			return false, false
		}

		tCmp, ok, dateOnlyCmp := parseDate(cmp)
		if !ok {
			return false, false
		}

		comparisons = append(comparisons, dateComparison{op: op, value: tCmp, dateOnly: dateOnlyCmp})
	}

	for _, c := range comparisons {
		tv, tc := tVal, c.value
		if dateOnlyVal || c.dateOnly {
			tv = tv.Truncate(24 * time.Hour)
			tc = tc.Truncate(24 * time.Hour)
		}

		switch c.op {
		case "eq":
			if !tv.Equal(tc) {
				return true, false
			}
		case "neq":
			if tv.Equal(tc) {
				return true, false
			}
		case "lt":
			if !tv.Before(tc) {
				return true, false
			}
		case "lte":
			if tv.After(tc) {
				return true, false
			}
		case "gt":
			if !tv.After(tc) {
				return true, false
			}
		case "gte":
			if tv.Before(tc) {
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
		case "lt":
			if !(value < cmp) {
				return false
			}
		case "lte":
			if !(value <= cmp) {
				return false
			}
		case "gt":
			if !(value > cmp) {
				return false
			}
		case "gte":
			if !(value >= cmp) {
				return false
			}
		case "contains":
			if !strings.Contains(strings.ToLower(value), strings.ToLower(cmp)) {
				return false
			}
		case "not_contains":
			if strings.Contains(strings.ToLower(value), strings.ToLower(cmp)) {
				return false
			}
		default:
			return false
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
		default:
			return false
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
