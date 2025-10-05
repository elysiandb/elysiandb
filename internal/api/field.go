package api_storage

import "strings"

func GetNestedValue(data map[string]interface{}, path string) (interface{}, bool) {
	parts := strings.Split(path, ".")
	var current interface{} = data
	for _, part := range parts {
		m, ok := current.(map[string]interface{})
		if !ok {
			return nil, false
		}
		current, ok = m[part]
		if !ok {
			return nil, false
		}
	}
	return current, true
}

func SetNestedField(dest map[string]interface{}, path string, value interface{}) {
	parts := strings.Split(path, ".")
	current := dest
	for i, part := range parts {
		if i == len(parts)-1 {
			current[part] = value
			return
		}
		if _, ok := current[part]; !ok {
			current[part] = make(map[string]interface{})
		}
		current = current[part].(map[string]interface{})
	}
}

func FilterFields(data map[string]interface{}, fields []string) map[string]interface{} {
	filtered := make(map[string]interface{})
	for _, field := range fields {
		if value, ok := GetNestedValue(data, field); ok {
			SetNestedField(filtered, field, value)
		}
	}
	return filtered
}

func ParseFieldsParam(param string) []string {
	if param == "" {
		return nil
	}
	parts := strings.Split(param, ",")
	fields := make([]string, 0, len(parts))
	for _, part := range parts {
		part = strings.TrimSpace(part)
		if part != "" {
			fields = append(fields, part)
		}
	}
	return fields
}
