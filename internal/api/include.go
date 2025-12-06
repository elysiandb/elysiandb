package api_storage

import "strings"

var ReadEntityByIdFunc = ReadEntityById

func ApplyIncludes(data []map[string]interface{}, includesParam string) []map[string]interface{} {
	allMode := strings.TrimSpace(includesParam) == "all"
	includeTree := buildIncludeTree(includesParam, allMode)
	for _, entityData := range data {
		applyIncludesRecursive(entityData, includeTree, allMode)
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

func applyIncludesRecursive(entityData map[string]interface{}, tree map[string][]string, forceAll bool) {
	fields := collectIncludeFields(entityData, tree, forceAll)
	for _, field := range fields {
		val, ok := entityData[field]
		if !ok || val == nil {
			continue
		}
		switch v := val.(type) {
		case map[string]interface{}:
			entityData[field] = includeSingleEntity(v)
			next := buildNextTree(tree[field], forceAll)
			if m, ok := entityData[field].(map[string]interface{}); ok {
				applyIncludesRecursive(m, next, forceAll)
			}
		case []interface{}:
			newList := []map[string]interface{}{}
			for _, item := range v {
				if m, ok := item.(map[string]interface{}); ok {
					m = includeSingleEntity(m)
					next := buildNextTree(tree[field], forceAll)
					applyIncludesRecursive(m, next, forceAll)
					newList = append(newList, m)
				}
			}
			entityData[field] = newList
		}
	}
}

func collectIncludeFields(entityData map[string]interface{}, tree map[string][]string, forceAll bool) []string {
	fields := []string{}
	if forceAll {
		for k, v := range entityData {
			switch val := v.(type) {
			case map[string]interface{}:
				if _, ok := val["@entity"]; ok {
					fields = append(fields, k)
				}
			case []interface{}:
				for _, item := range val {
					if m, ok := item.(map[string]interface{}); ok {
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

func includeSingleEntity(m map[string]interface{}) map[string]interface{} {
	if ent, ok := m["@entity"].(string); ok {
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
