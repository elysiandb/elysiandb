package api_storage

import (
	"maps"
	"strings"
)

type EntityReader func(entity, id string) map[string]any

type includedRef struct {
	entity string
	id     string
}

func ApplyIncludes(data []map[string]any, includesParam string, read EntityReader) []map[string]any {
	allMode := strings.TrimSpace(includesParam) == "all"
	includeTree := buildIncludeTree(includesParam, allMode)
	expanding := map[includedRef]struct{}{}
	for _, entityData := range data {
		applyIncludesRecursive(entityData, includeTree, allMode, read, expanding)
	}

	return data
}

func buildIncludeTree(includesParam string, allMode bool) map[string][]string {
	includeTree := make(map[string][]string)
	if allMode {
		return includeTree
	}

	includes := strings.Split(includesParam, ",")
	for _, inc := range includes {
		parts := strings.SplitN(strings.TrimSpace(inc), ".", 2)
		if len(parts) == 1 {
			includeTree[parts[0]] = append(includeTree[parts[0]], "")
		} else {
			includeTree[parts[0]] = append(includeTree[parts[0]], parts[1])
		}
	}

	return includeTree
}

func applyIncludesRecursive(
	entityData map[string]any,
	tree map[string][]string,
	forceAll bool,
	read EntityReader,
	expanding map[includedRef]struct{},
) {
	fields := collectIncludeFields(entityData, tree, forceAll)
	for _, field := range fields {
		val, ok := entityData[field]
		if !ok || val == nil {
			continue
		}

		switch v := val.(type) {
		case map[string]any:
			next := buildNextTree(tree[field], forceAll)
			entityData[field] = includeEntity(v, next, forceAll, read, expanding)
		case []any:
			newList := []map[string]any{}
			for _, item := range v {
				if m, ok := item.(map[string]any); ok {
					next := buildNextTree(tree[field], forceAll)
					newList = append(newList, includeEntity(m, next, forceAll, read, expanding))
				}
			}

			entityData[field] = newList
		}
	}
}

func includeEntity(
	m map[string]any,
	next map[string][]string,
	forceAll bool,
	read EntityReader,
	expanding map[includedRef]struct{},
) map[string]any {
	included, ref, ok := includeSingleEntity(m, forceAll, read, expanding)
	if !ok {
		included = maps.Clone(m)
		applyIncludesRecursive(included, next, forceAll, read, expanding)

		return included
	}

	expanding[ref] = struct{}{}
	applyIncludesRecursive(included, next, forceAll, read, expanding)
	delete(expanding, ref)

	return included
}

func collectIncludeFields(entityData map[string]any, tree map[string][]string, forceAll bool) []string {
	fields := []string{}
	if forceAll {
		for k, v := range entityData {
			switch val := v.(type) {
			case map[string]any:
				if _, ok := val["@entity"]; ok {
					fields = append(fields, k)
				}
			case []any:
				for _, item := range val {
					if m, ok := item.(map[string]any); ok {
						if _, ok2 := m["@entity"]; ok2 {
							fields = append(fields, k)
							break
						}
					}
				}
			}
		}
	} else {
		for k := range tree {
			fields = append(fields, k)
		}
	}

	return fields
}

func includeSingleEntity(
	m map[string]any,
	forceAll bool,
	read EntityReader,
	expanding map[includedRef]struct{},
) (map[string]any, includedRef, bool) {
	ent, ok := m["@entity"].(string)
	if !ok {
		return m, includedRef{}, false
	}

	id, ok := m["id"].(string)
	if !ok || id == "" {
		return m, includedRef{}, false
	}

	ref := includedRef{entity: ent, id: id}
	if _, cyclic := expanding[ref]; cyclic && forceAll {
		return m, includedRef{}, false
	}

	loaded := read(ent, id)
	if loaded == nil {
		return m, includedRef{}, false
	}

	loaded["@entity"] = ent

	return loaded, ref, true
}

func buildNextTree(subs []string, forceAll bool) map[string][]string {
	if forceAll {
		return map[string][]string{}
	}

	next := map[string][]string{}
	for _, sub := range subs {
		if sub != "" {
			parts := strings.SplitN(sub, ".", 2)
			if len(parts) == 1 {
				next[parts[0]] = append(next[parts[0]], "")
			} else {
				next[parts[0]] = append(next[parts[0]], parts[1])
			}
		}
	}

	return next
}

func ExtractAutoIncludes(filters map[string]map[string]string) string {
	set := map[string]struct{}{}
	for field := range filters {
		if strings.Contains(field, ".") {
			parts := strings.Split(field, ".")
			include := strings.Join(parts[:len(parts)-1], ".")
			set[include] = struct{}{}
		}
	}

	out := ""
	first := true
	for inc := range set {
		if first {
			out = inc
			first = false
		} else {
			out = out + "," + inc
		}
	}

	return out
}

func MergeIncludes(a, b string) string {
	if a == "" {
		return b
	}
	if b == "" {
		return a
	}
	seen := map[string]struct{}{}
	out := ""

	add := func(s string) {
		parts := strings.Split(s, ",")
		for _, p := range parts {
			p = strings.TrimSpace(p)
			if p == "" {
				continue
			}

			if _, ok := seen[p]; !ok {
				seen[p] = struct{}{}
				if out == "" {
					out = p
				} else {
					out = out + "," + p
				}
			}
		}
	}

	add(a)
	add(b)

	return out
}
