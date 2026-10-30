package api_storage

import (
	"cmp"
	"sort"
	"strings"
	"time"
)

const (
	sortRankEmptyArray = iota
	sortRankNull
	sortRankNumber
	sortRankString
	sortRankObject
	sortRankArray
	sortRankBool
	sortRankDate
	sortRankOther
)

type emptyArraySortKey struct{}

func getSortNestedValue(m map[string]any, path string) any {
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
		if t, ok := parseDateForSort(s); ok {
			return t
		}

		return s
	}

	return cur
}

func parseDateForSort(s string) (time.Time, bool) {
	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t, true
	}

	if t, err := time.Parse("2006-01-02", s); err == nil {
		return t, true
	}

	return time.Time{}, false
}

func SortKey(entity map[string]any, field string, ascending bool) any {
	value := getSortNestedValue(entity, field)
	arr, ok := sortArray(value)
	if !ok {
		return value
	}

	if len(arr) == 0 {
		return emptyArraySortKey{}
	}

	key := arr[0]
	for _, item := range arr[1:] {
		c := CompareSortValues(item, key)
		if (ascending && c < 0) || (!ascending && c > 0) {
			key = item
		}
	}

	return key
}

func CompareSortValues(a, b any) int {
	ra, rb := sortRank(a), sortRank(b)
	if ra != rb {
		return cmp.Compare(ra, rb)
	}

	switch ra {
	case sortRankNumber:
		fa, _ := toSortNumber(a)
		fb, _ := toSortNumber(b)
		return cmp.Compare(fa, fb)
	case sortRankString:
		return strings.Compare(a.(string), b.(string))
	case sortRankBool:
		ba, bb := a.(bool), b.(bool)
		if ba == bb {
			return 0
		}
		if !ba {
			return -1
		}
		return 1
	case sortRankDate:
		return cmp.Compare(a.(time.Time).UnixMilli(), b.(time.Time).UnixMilli())
	}

	return 0
}

func sortArray(v any) ([]any, bool) {
	switch t := v.(type) {
	case []any:
		return t, true
	case []map[string]any:
		items := make([]any, len(t))
		for i, item := range t {
			items[i] = item
		}

		return items, true
	}

	return nil, false
}

func sortRank(v any) int {
	switch v.(type) {
	case emptyArraySortKey:
		return sortRankEmptyArray
	case nil:
		return sortRankNull
	case string:
		return sortRankString
	case map[string]any:
		return sortRankObject
	case []any, []map[string]any:
		return sortRankArray
	case bool:
		return sortRankBool
	case time.Time:
		return sortRankDate
	}

	if _, ok := toSortNumber(v); ok {
		return sortRankNumber
	}

	return sortRankOther
}

func toSortNumber(v any) (float64, bool) {
	switch n := v.(type) {
	case float64:
		return n, true
	case float32:
		return float64(n), true
	case int:
		return float64(n), true
	case int32:
		return float64(n), true
	case int64:
		return float64(n), true
	}

	return 0, false
}

func SortEntitiesOrder(data []map[string]any, field string, ascending bool) []int {
	keys := make([]any, len(data))
	ids := make([]string, len(data))
	order := make([]int, len(data))
	for i, item := range data {
		keys[i] = SortKey(item, field, ascending)
		ids[i], _ = item["id"].(string)
		order[i] = i
	}

	sort.SliceStable(order, func(i, j int) bool {
		a, b := order[i], order[j]
		c := CompareSortValues(keys[a], keys[b])
		if c == 0 {
			return ids[a] < ids[b]
		}

		if ascending {
			return c < 0
		}

		return c > 0
	})

	return order
}

func GetSortedEntityIdsByField(entity, field string, ascending bool) []string {
	data := ListEntities(entity, 0, 0, "", ascending, map[string]map[string]string{}, "", "all")

	var ids []string
	for _, i := range SortEntitiesOrder(data, field, ascending) {
		if idVal, ok := data[i]["id"].(string); ok {
			ids = append(ids, idVal)
		}
	}

	return ids
}
