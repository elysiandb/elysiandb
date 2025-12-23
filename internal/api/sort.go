package api_storage

import (
	"sort"
	"strings"
	"time"
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
