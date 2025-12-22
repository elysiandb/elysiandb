package schema

import (
	"fmt"
	"reflect"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

const SchemaEntity = "_elysiandb_core_schema"

var LoadSchemaForEntity = loadSchemaForEntity

type Field struct {
	Name     string
	Type     string
	Required bool
	Fields   map[string]Field
}

type Entity struct {
	ID     string
	Fields map[string]Field
}

type ValidationError struct {
	Field   string
	Message string
}

func (v *ValidationError) ToError() error {
	return fmt.Errorf("Field '%s': %s", v.Field, v.Message)
}

func AnalyzeEntitySchema(entity string, data map[string]interface{}) map[string]interface{} {
	required := globals.GetConfig().Api.Schema.Strict
	schema := Entity{
		ID:     entity,
		Fields: analyzeFields(data, true, required),
	}

	return SchemaEntityToStorableStructure(schema)
}

func analyzeFields(data map[string]interface{}, isRoot, required bool) map[string]Field {
	fields := make(map[string]Field)
	for k, v := range data {
		if isRoot && k == "id" || strings.HasPrefix(k, globals.CoreFieldsPrefix) {
			continue
		}

		f := Field{Name: k, Required: required}
		typeName := DetectJSONType(v)
		f.Type = typeName
		switch typeName {
		case "object":
			sub, _ := v.(map[string]interface{})
			f.Fields = analyzeFields(sub, false, required)
		case "array":
			arr := v.([]interface{})
			if len(arr) > 0 {
				if firstObj, ok := arr[0].(map[string]interface{}); ok {
					f.Fields = analyzeFields(firstObj, false, required)
				}
			}
		}

		fields[k] = f
	}

	return fields
}

func DetectJSONType(v interface{}) string {
	switch val := v.(type) {
	case string:
		return "string"
	case float64, float32, int, int64, uint64, uint:
		return "number"
	case bool:
		return "boolean"
	case map[string]interface{}:
		return "object"
	case []interface{}:
		return "array"
	default:
		if val != nil {
			return reflect.TypeOf(val).String()
		}
		return "unknown"
	}
}

func ValidateEntity(entity string, data map[string]interface{}, schemaData map[string]any) []ValidationError {
	var errors []ValidationError
	s := LoadSchemaForEntity(entity, schemaData)
	if s == nil {
		return errors
	}

	isStrict := globals.GetConfig().Api.Schema.Strict && IsManualSchema(entity, schemaData)

	if isStrict {
		validateNoExtraFieldsRecursive(s.Fields, data, "", &errors)
	}

	validateFieldsRecursive(s.Fields, data, "", &errors, isStrict)

	return errors
}

func validateNoExtraFieldsRecursive(fields map[string]Field, data map[string]interface{}, prefix string, errors *[]ValidationError) {
	for key, val := range data {
		if key == "id" {
			continue
		}

		fieldDef, ok := fields[key]
		full := key
		if prefix != "" {
			full = prefix + "." + key
		}

		if !ok {
			*errors = append(*errors, ValidationError{
				Field:   full,
				Message: "field not allowed by strict schema",
			})
			continue
		}

		if fieldDef.Type == "object" {
			if sub, ok := val.(map[string]interface{}); ok {
				validateNoExtraFieldsRecursive(fieldDef.Fields, sub, full, errors)
			}
		}

		if fieldDef.Type == "array" && len(fieldDef.Fields) > 0 {
			if arr, ok := val.([]interface{}); ok {
				for i, item := range arr {
					if obj, ok := item.(map[string]interface{}); ok {
						validateNoExtraFieldsRecursive(fieldDef.Fields, obj, fmt.Sprintf("%s[%d]", full, i), errors)
					}
				}
			}
		}
	}
}

func validateFieldsRecursive(fields map[string]Field, data map[string]interface{}, prefix string, errors *[]ValidationError, strict bool) {
	for fieldName, fieldDef := range fields {
		if fieldName == "id" {
			continue
		}

		fullName := fieldName
		if prefix != "" {
			fullName = prefix + "." + fieldName
		}

		value, exists := data[fieldName]

		if !exists {
			if strict && fieldDef.Required {
				*errors = append(*errors, ValidationError{
					Field:   fullName,
					Message: "required field missing",
				})
			}

			continue
		}

		expected := fieldDef.Type
		actual := DetectJSONType(value)
		if actual != expected {
			*errors = append(*errors, ValidationError{
				Field:   fullName,
				Message: fmt.Sprintf("expected type %s but got %s", expected, actual),
			})
			continue
		}

		if expected == "object" {
			sub, ok := value.(map[string]interface{})
			if !ok {
				*errors = append(*errors, ValidationError{
					Field:   fullName,
					Message: "expected object",
				})
				continue
			}

			validateFieldsRecursive(fieldDef.Fields, sub, fullName, errors, strict)
		}

		if expected == "array" {
			arr, ok := value.([]interface{})
			if !ok {
				*errors = append(*errors, ValidationError{
					Field:   fullName,
					Message: "expected array",
				})
				continue
			}
			if len(fieldDef.Fields) > 0 {
				for i, item := range arr {
					if obj, ok := item.(map[string]interface{}); ok {
						validateFieldsRecursive(fieldDef.Fields, obj, fmt.Sprintf("%s[%d]", fullName, i), errors, strict)
					}
				}
			}
		}
	}
}

func IsManualSchema(entity string, schemaData map[string]any) bool {
	if schemaData == nil {
		key := globals.ApiSingleEntityKey(SchemaEntity, entity)
		schemaData, _ = storage.GetJsonByKey(key)
	}

	if schemaData == nil {
		return false
	}

	_, ok := schemaData["_manual"]

	return ok
}

func SchemaEntityToStorableStructure(entity Entity) map[string]interface{} {
	return map[string]interface{}{
		"id":     entity.ID,
		"fields": FieldsToMap(entity.Fields),
	}
}

func FieldsToMap(fields map[string]Field) map[string]interface{} {
	out := make(map[string]interface{})
	for _, v := range fields {
		fieldMap := map[string]interface{}{
			"name":     v.Name,
			"type":     v.Type,
			"required": v.Required,
		}

		if len(v.Fields) > 0 {
			fieldMap["fields"] = FieldsToMap(v.Fields)
		}

		out[v.Name] = fieldMap
	}

	return out
}

func MapToFields(m map[string]interface{}) map[string]Field {
	fields := make(map[string]Field)
	for k, v := range m {
		if fieldMap, ok := v.(map[string]interface{}); ok {
			f := Field{Name: k}
			if typeName, ok := fieldMap["type"].(string); ok {
				f.Type = typeName
			}

			if req, ok := fieldMap["required"].(bool); ok {
				f.Required = req
			}

			if subFields, ok := fieldMap["fields"].(map[string]interface{}); ok {
				f.Fields = MapToFields(subFields)
			}

			if name, ok := fieldMap["name"].(string); ok {
				f.Name = name
				fields[name] = f
			} else {
				fields[k] = f
			}
		}
	}

	return fields
}

func loadSchemaForEntity(entity string, schemaData map[string]any) *Entity {
	if schemaData == nil {
		key := globals.ApiSingleEntityKey(SchemaEntity, entity)
		raw, _ := storage.GetJsonByKey(key)
		if raw == nil {
			return nil
		}
		schemaData = normalizeAnyMap(raw)
	}

	schema := &Entity{ID: entity}

	if fields, ok := schemaData["fields"].(map[string]any); ok {
		schema.Fields = MapToFields(fields)
	}

	return schema
}

func normalizeAnyMap(m map[string]interface{}) map[string]any {
	out := make(map[string]any, len(m))
	for k, v := range m {
		if sub, ok := v.(map[string]interface{}); ok {
			out[k] = normalizeAnyMap(sub)
		} else {
			out[k] = v
		}
	}
	return out
}
