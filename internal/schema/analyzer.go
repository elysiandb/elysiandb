package schema

import (
	"fmt"
	"reflect"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

const SchemaEntity = "schema"

var LoadSchemaForEntity = loadSchemaForEntity

type Field struct {
	Name   string
	Type   string
	Fields map[string]Field
}

type Entity struct {
	ID     string
	Fields map[string]Field
}

type ValidationError struct {
	Field   string
	Message string
}

func AnalyzeEntitySchema(entity string, data map[string]interface{}) map[string]interface{} {
	schema := Entity{
		ID:     entity,
		Fields: analyzeFields(data, true),
	}

	return schemaEntityToStorableStructure(schema)
}

func analyzeFields(data map[string]interface{}, isRoot bool) map[string]Field {
	fields := make(map[string]Field)

	for k, v := range data {
		if isRoot && k == "id" {
			continue
		}

		f := Field{Name: k}
		typeName := detectJSONType(v)
		f.Type = typeName

		switch typeName {

		case "object":
			sub, _ := v.(map[string]interface{})
			f.Fields = analyzeFields(sub, false)

		case "array":
			arr := v.([]interface{})
			if len(arr) > 0 {
				if firstObj, ok := arr[0].(map[string]interface{}); ok {
					f.Fields = analyzeFields(firstObj, false)
				}
			}
		}

		fields[k] = f
	}

	return fields
}

func detectJSONType(v interface{}) string {
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

func ValidateEntity(entity string, data map[string]interface{}) []ValidationError {
	var errors []ValidationError

	schema := LoadSchemaForEntity(entity)
	if schema == nil {
		return errors
	}

	validateFieldsRecursive(schema.Fields, data, "", &errors)
	return errors
}

func validateFieldsRecursive(fields map[string]Field, data map[string]interface{}, prefix string, errors *[]ValidationError) {
	for fieldName, fieldDef := range fields {

		fullName := fieldName
		if prefix != "" {
			fullName = prefix + "." + fieldName
		}

		value, exists := data[fieldName]
		if !exists {
			continue
		}

		expected := fieldDef.Type
		actual := detectJSONType(value)

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
			validateFieldsRecursive(fieldDef.Fields, sub, fullName, errors)
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
						validateFieldsRecursive(fieldDef.Fields, obj, fmt.Sprintf("%s[%d]", fullName, i), errors)
					}
				}
			}
		}
	}
}

func schemaEntityToStorableStructure(entity Entity) map[string]interface{} {
	return map[string]interface{}{
		"id":     entity.ID,
		"fields": fieldsToMap(entity.Fields),
	}
}

func fieldsToMap(fields map[string]Field) map[string]interface{} {
	out := make(map[string]interface{})
	for k, v := range fields {
		fieldMap := map[string]interface{}{
			"name": v.Name,
			"type": v.Type,
		}

		if len(v.Fields) > 0 {
			fieldMap["fields"] = fieldsToMap(v.Fields)
		}

		out[k] = fieldMap
	}

	return out
}

func mapToFields(m map[string]interface{}) map[string]Field {
	fields := make(map[string]Field)
	for k, v := range m {
		if fieldMap, ok := v.(map[string]interface{}); ok {
			f := Field{Name: k}

			if typeName, ok := fieldMap["type"].(string); ok {
				f.Type = typeName
			}

			if subFields, ok := fieldMap["fields"].(map[string]interface{}); ok {
				f.Fields = mapToFields(subFields)
			}

			fields[k] = f
		}
	}

	return fields
}

func loadSchemaForEntity(entity string) *Entity {
	key := globals.ApiSingleEntityKey(SchemaEntity, entity)
	data, _ := storage.GetJsonByKey(key)

	schema := &Entity{ID: entity}
	if fieldsMap, ok := data["fields"].(map[string]interface{}); ok {
		schema.Fields = mapToFields(fieldsMap)
	}

	return schema
}
