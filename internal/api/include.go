package api_storage

import "strings"

const maxIncludeDepth = 32

var ReadEntityByIdFunc = ReadEntityById

func ApplyIncludes(data []map[string]any, includesParam string) []map[string]any {
	allMode := strings.TrimSpace(includesParam) == "all"
	includeTree := buildIncludeTree(includesParam, allMode)
	for _, entityData := range data {
		applyIncludesRecursive(entityData, includeTree, allMode, map[string]bool{}, 0)
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

func applyIncludesRecursive(entityData map[string]any, tree map[string][]string, forceAll bool, ancestors map[string]bool, depth int) {
	if depth >= maxIncludeDepth {
		return
	}

	fields := collectIncludeFields(entityData, tree, forceAll)
	for _, field := range fields {
		val, ok := entityData[field]
		if !ok || val == nil {
			continue
		}

		switch v := val.(type) {
		case map[string]any:
			entityData[field] = includeAndDescend(v, tree[field], forceAll, ancestors, depth)
		case []any:
			newList := []map[string]any{}
			for _, item := range v {
				if m, ok := item.(map[string]any); ok {
					newList = append(newList, includeAndDescend(m, tree[field], forceAll, ancestors, depth))
				}
			}

			entityData[field] = newList
		}
	}
}

func includeAndDescend(m map[string]any, subs []string, forceAll bool, ancestors map[string]bool, depth int) map[string]any {
	ref := includeReference(m)
	if ref != "" && ancestors[ref] {
		return m
	}

	included := includeSingleEntity(m)
	if ref != "" {
		ancestors[ref] = true
		defer delete(ancestors, ref)
	}

	applyIncludesRecursive(included, buildNextTree(subs, forceAll), forceAll, ancestors, depth+1)

	return included
}

func includeReference(m map[string]any) string {
	ent, _ := m["@entity"].(string)
	id, _ := m["id"].(string)
	if ent == "" || id == "" {
		return ""
	}

	return ent + "\x00" + id
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

func includeSingleEntity(m map[string]any) map[string]any {
	if ent, ok := m["@entity"].(string); ok && !IsCoreEntityType(ent) {
		if id, ok := m["id"].(string); ok && id != "" {
			read := ReadEntityByIdFunc(ent, id)
			if read != nil {
				read["@entity"] = ent
				return read
			}
		}
	}

	return m
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
