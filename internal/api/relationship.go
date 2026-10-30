package api_storage

import (
	"errors"
	"fmt"
	"strings"

	"github.com/google/uuid"
)

var ErrCoreSubEntity = errors.New("core entity types can only be referenced from other entities")

type SubEntityVisitor func(entity, id string, sub map[string]any) error

func ExtractSubEntities(entity string, data map[string]any) []map[string]any {
	var subs []map[string]any
	for k, v := range data {
		switch val := v.(type) {
		case map[string]any:
			subs = append(subs, handleMapSubEntity(entity, k, val, data)...)
		case []any:
			subs = append(subs, handleArraySubEntities(entity, k, val, data)...)
		case []map[string]any:
			items := make([]any, len(val))
			for i, item := range val {
				items[i] = item
			}

			subs = append(subs, handleArraySubEntities(entity, k, items, data)...)
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
		id, realFields := extractIDAndCheckFields(m)
		link := map[string]any{"@entity": subEntityName, "id": id}
		if !realFields && id != "" {
			return link, subs
		}

		sub := buildSubEntity(subEntityName, id, m)
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
	curID := providedSubEntityID(val)
	if curID == "" {
		curID = uuid.New().String()
	}

	return curID, hasRealFields(val)
}

func providedSubEntityID(val map[string]any) string {
	if s, ok := val["id"].(string); ok && s != "" {
		return s
	}

	if s, ok := val["@id"].(string); ok && s != "" {
		return s
	}

	return ""
}

func hasRealFields(val map[string]any) bool {
	for subKey := range val {
		if !isLinkKey(subKey) {
			return true
		}
	}

	return false
}

func isLinkKey(key string) bool {
	return key == "@entity" || key == "id" || key == "@id"
}

func buildSubEntity(subEntityName, id string, val map[string]any) map[string]any {
	sub := map[string]any{"id": id, "@entity": subEntityName}
	for subKey, subVal := range val {
		if !isLinkKey(subKey) {
			sub[subKey] = subVal
		}
	}

	return sub
}

func VisitSubEntityWrites(data map[string]any, visit SubEntityVisitor) error {
	for _, v := range data {
		if err := visitSubEntityValue(v, visit); err != nil {
			return err
		}
	}

	return nil
}

func visitSubEntityValue(v any, visit SubEntityVisitor) error {
	switch val := v.(type) {
	case map[string]any:
		return visitSubEntityMap(val, visit)
	case []any:
		for _, item := range val {
			if m, ok := item.(map[string]any); ok {
				if err := visitSubEntityMap(m, visit); err != nil {
					return err
				}
			}
		}
	case []map[string]any:
		for _, m := range val {
			if err := visitSubEntityMap(m, visit); err != nil {
				return err
			}
		}
	}

	return nil
}

func visitSubEntityMap(m map[string]any, visit SubEntityVisitor) error {
	subEntityName, ok := m["@entity"].(string)
	if !ok || subEntityName == "" {
		return VisitSubEntityWrites(m, visit)
	}

	if !hasRealFields(m) {
		return nil
	}

	if err := visit(subEntityName, providedSubEntityID(m), m); err != nil {
		return err
	}

	for k, v := range m {
		if isLinkKey(k) {
			continue
		}

		if err := visitSubEntityValue(v, visit); err != nil {
			return err
		}
	}

	return nil
}

func RejectCoreSubEntities(data map[string]any) error {
	return VisitSubEntityWrites(data, func(entity, _ string, _ map[string]any) error {
		return CheckSubEntityType(entity)
	})
}

func CheckSubEntityType(entity string) error {
	if strings.HasPrefix(entity, CoreEntityTypePrefix) {
		return fmt.Errorf("%w: %s", ErrCoreSubEntity, entity)
	}

	return nil
}

func ReferenceCoreSubEntities(data map[string]any) {
	for k, v := range data {
		if ref, ok := coreSubEntityReference(v); ok {
			data[k] = ref
			continue
		}

		referenceCoreSubEntitiesIn(v)
	}
}

func referenceCoreSubEntitiesIn(v any) {
	switch val := v.(type) {
	case map[string]any:
		ReferenceCoreSubEntities(val)
	case []any:
		for i, item := range val {
			if ref, ok := coreSubEntityReference(item); ok {
				val[i] = ref
				continue
			}

			referenceCoreSubEntitiesIn(item)
		}
	case []map[string]any:
		for i, item := range val {
			if ref, ok := coreSubEntityReference(item); ok {
				val[i] = ref
				continue
			}

			ReferenceCoreSubEntities(item)
		}
	}
}

func coreSubEntityReference(v any) (map[string]any, bool) {
	m, ok := v.(map[string]any)
	if !ok {
		return nil, false
	}

	subEntityName, ok := m["@entity"].(string)
	if !ok || CheckSubEntityType(subEntityName) == nil || !hasRealFields(m) {
		return nil, false
	}

	return map[string]any{"@entity": subEntityName, "id": providedSubEntityID(m)}, true
}
