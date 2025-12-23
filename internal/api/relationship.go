package api_storage

import (
	"github.com/google/uuid"
)

func ExtractSubEntities(entity string, data map[string]any) []map[string]any {
	var subs []map[string]any
	for k, v := range data {
		switch val := v.(type) {
		case map[string]any:
			subs = append(subs, handleMapSubEntity(entity, k, val, data)...)
		case []any:
			subs = append(subs, handleArraySubEntities(entity, k, val, data)...)
		}
	}

	return subs
}

func handleMapSubEntity(entity, key string, val, data map[string]any) []map[string]any {
	var subs []map[string]any
	if subEntityName, ok := val["@entity"].(string); ok && subEntityName != "" {
		id, realFields := extractIDAndCheckFields(val)
		if !realFields && id != "" {
			data[key] = map[string]any{"@entity": subEntityName, "id": id}
			return subs
		}

		sub := buildSubEntity(subEntityName, id, val)
		data[key] = map[string]any{"@entity": subEntityName, "id": id}
		deeper := ExtractSubEntities(subEntityName, sub)
		if len(deeper) > 0 {
			subs = append(subs, deeper...)
		}

		subs = append(subs, sub)
	} else {
		deeper := ExtractSubEntities(entity, val)
		if len(deeper) > 0 {
			subs = append(subs, deeper...)
		}

		data[key] = val
	}

	return subs
}

func handleArraySubEntities(entity, key string, arr []any, data map[string]any) []map[string]any {
	var subs []map[string]any
	newArr := make([]interface{}, len(arr))
	for i, item := range arr {
		if m, ok := item.(map[string]interface{}); ok {
			newArr[i], subs = processArrayItem(entity, m, subs)
		} else {
			newArr[i] = item
		}
	}

	data[key] = newArr

	return subs
}

func processArrayItem(entity string, m map[string]any, subs []map[string]any) (any, []map[string]any) {
	if subEntityName, ok := m["@entity"].(string); ok && subEntityName != "" {
		id, _ := extractIDAndCheckFields(m)
		sub := buildSubEntity(subEntityName, id, m)
		link := map[string]any{"@entity": subEntityName, "id": id}
		deeper := ExtractSubEntities(subEntityName, sub)
		if len(deeper) > 0 {
			subs = append(subs, deeper...)
		}

		subs = append(subs, sub)

		return link, subs
	}

	deeper := ExtractSubEntities(entity, m)
	if len(deeper) > 0 {
		subs = append(subs, deeper...)
	}

	return m, subs
}

func extractIDAndCheckFields(val map[string]any) (string, bool) {
	var curID string
	if s, ok := val["id"].(string); ok && s != "" {
		curID = s
	} else if s, ok := val["@id"].(string); ok && s != "" {
		curID = s
	}

	realFields := false
	for subKey := range val {
		if subKey != "@entity" && subKey != "id" && subKey != "@id" {
			realFields = true
			break
		}
	}

	if curID == "" {
		curID = uuid.New().String()
	}

	return curID, realFields
}

func buildSubEntity(subEntityName, id string, val map[string]any) map[string]any {
	sub := map[string]any{"id": id, "@entity": subEntityName}
	for subKey, subVal := range val {
		if subKey != "@entity" && subKey != "id" && subKey != "@id" {
			sub[subKey] = subVal
		}
	}

	return sub
}
