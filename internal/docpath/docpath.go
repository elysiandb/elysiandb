package docpath

import (
	"strings"
	"time"
)

const CoreFieldsPrefix = "_elysiandb_core_"

func Split(path string) []string {
	out := []string{}
	start := 0
	for i := 0; i <= len(path); i++ {
		if i == len(path) || path[i] == '.' {
			out = append(out, path[start:i])
			start = i + 1
		}
	}

	return out
}

func ResolveValues(data any, path string) []any {
	return resolve(data, Split(path))
}

func resolve(current any, parts []string) []any {
	if len(parts) == 0 {
		return []any{current}
	}

	switch v := current.(type) {
	case map[string]any:
		next, ok := v[parts[0]]
		if !ok {
			return nil
		}

		return resolve(next, parts[1:])
	case []any:
		var out []any
		for _, item := range v {
			out = append(out, resolve(item, parts)...)
		}

		return out
	case []map[string]any:
		var out []any
		for _, item := range v {
			out = append(out, resolve(item, parts)...)
		}

		return out
	default:
		return nil
	}
}

func SortValue(m map[string]any, path string) any {
	parts := strings.Split(path, ".")
	var cur any = m
	for _, p := range parts {
		if mm, ok := cur.(map[string]any); ok {
			cur = mm[p]
		} else {
			return nil
		}
	}

	if s, ok := cur.(string); ok {
		if t, ok := ParseSortDate(s); ok {
			return t
		}

		return s
	}

	return cur
}

func ParseSortDate(s string) (time.Time, bool) {
	if !looksLikeDate(s) {
		return time.Time{}, false
	}

	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t, true
	}

	if t, err := time.Parse("2006-01-02", s); err == nil {
		return t, true
	}

	return time.Time{}, false
}

func ParseDate(s string) (time.Time, bool, bool) {
	if !looksLikeDate(s) {
		return time.Time{}, false, false
	}

	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t, true, false
	}

	if t, err := time.Parse("2006-01-02", s); err == nil {
		return t, true, true
	}

	return time.Time{}, false, false
}

func looksLikeDate(s string) bool {
	if len(s) < 10 {
		return false
	}

	for i := 0; i < 4; i++ {
		if s[i] < '0' || s[i] > '9' {
			return false
		}
	}

	return s[4] == '-'
}

func IsSearchableKey(key string) bool {
	return key != "id" && key != "@entity" && !strings.HasPrefix(key, CoreFieldsPrefix)
}

func MatchGlob(pattern, s string) bool {
	pattern = strings.ToLower(pattern)
	s = strings.ToLower(s)
	p := pattern
	i, j := 0, 0
	star := -1
	match := 0

	for j < len(s) {
		if i < len(p) && p[i] == '\\' {
			if i+1 >= len(p) {
				return false
			}

			if p[i+1] != s[j] {
				if star != -1 {
					i = star + 1
					match++
					j = match

					continue
				}

				return false
			}

			i += 2
			j++

			continue
		}

		if i < len(p) && p[i] == '*' {
			star = i
			i++
			match = j

			continue
		}

		if i < len(p) && p[i] == s[j] {
			i++
			j++

			continue
		}

		if star != -1 {
			i = star + 1
			match++
			j = match

			continue
		}

		return false
	}

	for i < len(p) {
		if p[i] == '\\' {
			return false
		}

		if p[i] != '*' {
			return false
		}

		i++
	}

	return true
}
