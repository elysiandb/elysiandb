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
		Fields: analyzeFields(data),
	}

	return schemaEntityToStorableStructure(schema)
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

		t := reflect.TypeOf(value)
		if t == nil {
			*errors = append(*errors, ValidationError{
				Field:   fullName,
				Message: "nil value not allowed",
			})
			continue
		}

		typeName := t.String()
		if typeName != fieldDef.Type {
			*errors = append(*errors, ValidationError{
				Field:   fullName,
				Message: fmt.Sprintf("expected type %s but got %s", fieldDef.Type, typeName),
			})
			continue
		}

		if len(fieldDef.Fields) > 0 {
			subMap, ok := value.(map[string]interface{})
			if !ok {
				*errors = append(*errors, ValidationError{
					Field:   fullName,
					Message: "expected nested object",
				})
				continue
			}
			validateFieldsRecursive(fieldDef.Fields, subMap, fullName, errors)
		}
	}
}

func analyzeFields(data map[string]interface{}) map[string]Field {
	fields := make(map[string]Field)
	for k, v := range data {
		if k == "id" {
			continue
		}
		t := reflect.TypeOf(v)
		typeName := "unknown"
		if t != nil {
			typeName = t.String()
		}
		f := Field{Name: k, Type: typeName}
		if sub, ok := v.(map[string]interface{}); ok {
			f.Fields = analyzeFields(sub)
		}
		fields[k] = f
	}
	return fields
}

func schemaEntityToStorableStructure(entity Entity) map[string]interface{} {
	result := make(map[string]interface{})
	result["id"] = entity.ID
	result["fields"] = fieldsToMap(entity.Fields)
	return result
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
