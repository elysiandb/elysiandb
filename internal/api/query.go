package api_storage

import (
	"strings"
)

type Query struct {
	Entity string
	Offset int
	Limit  int
	Filter FilterNode
	Sorts  map[string]string
}

type FilterNode struct {
	Or   []FilterNode
	And  []FilterNode
	Leaf map[string]map[string]string
}

func ExecuteQuery(q Query) ([]map[string]any, error) {
	sortField := ""
	sortAsc := true

	if len(q.Sorts) > 0 {
		for field, dir := range q.Sorts {
			sortField = field
			if strings.ToLower(dir) == "desc" {
				sortAsc = false
			}
			break
		}
	}

	data := ListEntities(
		q.Entity,
		0,
		0,
		sortField,
		sortAsc,
		nil,
		"",
		"",
	)

	filtered := ApplyQueryFilter(data, q.Filter)

	return applyOffsetLimit(filtered, q.Offset, q.Limit), nil
}

func ApplyQueryFilter(data []map[string]any, filter FilterNode) []map[string]any {
	filtered := make([]map[string]any, 0, len(data))
	for _, e := range data {
		if matchFilterNode(filter, e) {
			filtered = append(filtered, e)
		}
	}

	return filtered
}

func matchFilterNode(node FilterNode, entity map[string]any) bool {
	if node.Leaf != nil {
		return matchLeafStrict(entity, node.Leaf)
	}

	if len(node.And) > 0 {
		for _, n := range node.And {
			if !matchFilterNode(n, entity) {
				return false
			}
		}
		return true
	}

	if len(node.Or) > 0 {
		for _, n := range node.Or {
			if matchFilterNode(n, entity) {
				return true
			}
		}
		return false
	}

	return false
}

func matchLeafStrict(entity map[string]any, filters map[string]map[string]string) bool {
	for field, ops := range filters {
		values := resolveValues(entity, field)
		if len(values) == 0 {
			return false
		}

		matched := false
		for _, val := range values {
			if matchValue(val, ops) {
				matched = true
				break
			}
		}

		if !matched {
			return false
		}
	}

	return true
}

func matchValue(val any, ops map[string]string) bool {
	switch v := val.(type) {
	case string:
		return matchStringOrDate(v, ops)
	case float64:
		return matchNumber(v, ops)
	case []any:
		return matchArray(v, ops)
	case bool:
		return matchBoolean(v, ops)
	default:
		return false
	}
}

func resolveValues(data any, path string) []any {
	parts := splitPath(path)
	return resolveRecursive(data, parts)
}

func resolveRecursive(current any, parts []string) []any {
	if len(parts) == 0 {
		return []any{current}
	}

	switch v := current.(type) {
	case map[string]any:
		next, ok := v[parts[0]]
		if !ok {
			return nil
		}
		return resolveRecursive(next, parts[1:])
	case []any:
		var out []any
		for _, item := range v {
			out = append(out, resolveRecursive(item, parts)...)
		}
		return out
	default:
		return nil
	}
}

func splitPath(path string) []string {
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
