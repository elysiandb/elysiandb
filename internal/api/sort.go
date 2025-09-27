package api_storage

import (
	"sort"
	"strings"
)

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
	return cur
}

func GetSortedEntityIdsByField(entity string, field string, ascending bool) []string {
	data := ListEntities(entity, 0, 0, "", true, map[string]map[string]string{})

	sort.Slice(data, func(i, j int) bool {
		a := getSortNestedValue(data[i], field)
		b := getSortNestedValue(data[j], field)

		switch va := a.(type) {
		case int:
			vb, _ := b.(int)
			if ascending {
				return va < vb
			}
			return va > vb
		case float64:
			vb, _ := b.(float64)
			if ascending {
				return va < vb
			}
			return va > vb
		case string:
			vb, _ := b.(string)
			if ascending {
				return va < vb
			}
			return va > vb
		default:
			return true
		}
	})

	var ids []string
	for _, item := range data {
		if idVal, ok := item["id"].(string); ok {
			ids = append(ids, idVal)
		}
	}

	return ids
}
