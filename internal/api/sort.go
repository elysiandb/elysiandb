package api_storage

import (
	"sort"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/docpath"
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
	return docpath.SortValue(m, path)
}
