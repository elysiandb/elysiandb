package api_storage

import (
	"sort"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/query"
)

func SortEntities(data []map[string]any, sorts []query.Sort) {
	sort.SliceStable(data, func(i, j int) bool {
		for _, s := range sorts {
			a := getSortNestedValue(data[i], s.Field)
			b := getSortNestedValue(data[j], s.Field)

			if a == nil || b == nil {
				if a == nil && b == nil {
					continue
				}

				return b == nil
			}

			c := compareSortValues(a, b)
			if c == 0 {
				continue
			}

			if s.Ascending {
				return c < 0
			}

			return c > 0
		}

		return false
	})
}

func compareSortValues(a, b any) int {
	rankA, rankB := sortTypeRank(a), sortTypeRank(b)
	if rankA != rankB {
		return rankA - rankB
	}

	switch va := a.(type) {
	case time.Time:
		return va.Compare(b.(time.Time))
	case string:
		return strings.Compare(va, b.(string))
	case bool:
		vb := b.(bool)
		switch {
		case va == vb:
			return 0
		case !va:
			return -1
		default:
			return 1
		}
	}

	if na, ok := sortNumber(a); ok {
		nb, _ := sortNumber(b)
		switch {
		case na < nb:
			return -1
		case na > nb:
			return 1
		}
	}

	return 0
}

func sortTypeRank(v any) int {
	if _, ok := sortNumber(v); ok {
		return 0
	}

	switch v.(type) {
	case time.Time:
		return 1
	case string:
		return 2
	case bool:
		return 3
	default:
		return 4
	}
}

func sortNumber(v any) (float64, bool) {
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
	default:
		return 0, false
	}
}

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

func GetSortedEntityIdsByField(entity, field string, ascending bool) []string {
	data := ListEntities(entity, 0, 0, "", ascending, map[string]map[string]string{}, "", "all")

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
		case time.Time:
			switch vb := b.(type) {
			case time.Time:
				if ascending {
					return va.Before(vb)
				}

				return va.After(vb)
			case string:
				if tb, ok := parseDateForSort(vb); ok {
					if ascending {
						return va.Before(tb)
					}

					return va.After(tb)
				}

				if ascending {
					return true
				}

				return false
			default:
				if ascending {
					return true
				}

				return false
			}
		case string:
			if ta, ok := parseDateForSort(va); ok {
				switch vb := b.(type) {
				case time.Time:
					if ascending {
						return ta.Before(vb)
					}

					return ta.After(vb)
				case string:
					if tb, ok2 := parseDateForSort(vb); ok2 {
						if ascending {
							return ta.Before(tb)
						}

						return ta.After(tb)
					}
				}
			}

			vb, _ := b.(string)
			if ascending {
				return va < vb
			}

			return va > vb
		default:
			return false
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
