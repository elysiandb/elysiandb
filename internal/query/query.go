package query

import (
	"sort"
	"strings"
)

type Query struct {
	Entity    string
	Offset    int
	Limit     int
	Filter    FilterNode
	Sorts     map[string]string
	SortOrder []string
}

type FilterNode struct {
	Or   []FilterNode
	And  []FilterNode
	Leaf map[string]map[string]string
}

type Sort struct {
	Field     string
	Ascending bool
}

func (q Query) OrderedSorts() []Sort {
	fields := make([]string, 0, len(q.Sorts))
	seen := make(map[string]bool, len(q.Sorts))

	for _, field := range q.SortOrder {
		if _, ok := q.Sorts[field]; ok && !seen[field] {
			seen[field] = true
			fields = append(fields, field)
		}
	}

	remaining := make([]string, 0, len(q.Sorts))
	for field := range q.Sorts {
		if !seen[field] {
			remaining = append(remaining, field)
		}
	}

	sort.Strings(remaining)
	fields = append(fields, remaining...)

	sorts := make([]Sort, 0, len(fields))
	for _, field := range fields {
		sorts = append(sorts, Sort{
			Field:     field,
			Ascending: strings.ToLower(q.Sorts[field]) != "desc",
		})
	}

	return sorts
}
