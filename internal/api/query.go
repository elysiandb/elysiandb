package api_storage

import (
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/query"
)

func ExecuteQuery(q query.Query) ([]map[string]any, error) {
	return PlanQuery(q, false).Decode(), nil
}

func ExecuteQueryGuarded(q query.Query, guard *Guard) ([]map[string]any, error) {
	res := PlanQueryGuarded(q, false, guard)
	if res.Err != nil {
		return []map[string]any{}, res.Err
	}

	return res.Decode(), guard.Err()
}

func PlanQuery(q query.Query, countOnly bool) *ListResult {
	return PlanQueryGuarded(q, countOnly, nil)
}

func StreamQueryGuarded(q query.Query, guard *Guard, snap *datastore.Snapshot, fn func(datastore.Doc) bool) error {
	filter := q.Filter

	return Plan(ListQuery{
		Entity:   q.Entity,
		Sorts:    q.OrderedSorts(),
		Node:     &filter,
		Guard:    guard,
		Snapshot: snap,
		Each:     fn,
	}).Err
}

func PlanQueryGuarded(q query.Query, countOnly bool, guard *Guard) *ListResult {
	filter := q.Filter

	return Plan(ListQuery{
		Entity:    q.Entity,
		Offset:    q.Offset,
		Limit:     q.Limit,
		Sorts:     q.OrderedSorts(),
		Node:      &filter,
		CountOnly: countOnly,
		Guard:     guard,
	})
}

func ApplyQueryFilter(data []map[string]any, filter query.FilterNode) []map[string]any {
	filtered := make([]map[string]any, 0, len(data))
	for _, e := range data {
		if matchFilterNode(filter, e) {
			filtered = append(filtered, e)
		}
	}

	return filtered
}

func QueryFilterIsEmpty(filter query.FilterNode) bool {
	return filter.Leaf != nil && len(filter.Leaf) == 0 && len(filter.And) == 0 && len(filter.Or) == 0
}

func QueryFilterMatchesEntity(filter query.FilterNode, entity map[string]any) bool {
	return matchFilterNode(filter, entity)
}

func ApplyStoredQueryFilter(data []map[string]any, filter query.FilterNode) []map[string]any {
	filtered := make([]map[string]any, 0, len(data))
	for _, e := range data {
		if matchFilterNodeWith(filter, e, StoredFiltersMatchEntity) {
			filtered = append(filtered, e)
		}
	}

	return filtered
}

func matchFilterNode(node query.FilterNode, entity map[string]any) bool {
	return matchFilterNodeWith(node, entity, matchLeafStrict)
}

func matchFilterNodeWith(
	node query.FilterNode,
	entity map[string]any,
	matchLeaf func(map[string]any, map[string]map[string]string) bool,
) bool {
	if node.Leaf != nil {
		return matchLeaf(entity, node.Leaf)
	}

	if len(node.And) > 0 {
		for _, n := range node.And {
			if !matchFilterNodeWith(n, entity, matchLeaf) {
				return false
			}
		}

		return true
	}

	if len(node.Or) > 0 {
		for _, n := range node.Or {
			if matchFilterNodeWith(n, entity, matchLeaf) {
				return true
			}
		}

		return false
	}

	return false
}

func matchLeafStrict(entity map[string]any, filters map[string]map[string]string) bool {
	for field, ops := range filters {
		if !matchAnyValue(resolveValues(entity, field), ops) {
			return false
		}
	}

	return true
}

func matchAnyValue(values []any, ops map[string]string) bool {
	for _, val := range values {
		if matchValue(val, ops) {
			return true
		}
	}

	return false
}

func matchValue(val any, ops map[string]string) bool {
	switch v := val.(type) {
	case string:
		return matchStringOrDate(v, ops)
	case float64:
		return matchNumber(v, ops)
	case int:
		return matchNumber(float64(v), ops)
	case int32:
		return matchNumber(float64(v), ops)
	case int64:
		return matchNumber(float64(v), ops)
	case []any:
		return matchArray(v, ops)
	case bool:
		return matchBoolean(v, ops)
	default:
		return false
	}
}

func resolveValues(data any, path string) []any {
	return docpath.ResolveValues(data, path)
}
