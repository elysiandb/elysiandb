package api_storage

import (
	"maps"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
)

type WriteSimulation struct {
	loadSchema func(entity string) map[string]any
	loadEntity func(entity, id string) map[string]any
	schemas    map[string]map[string]any
	entities   map[string]map[string]any
}

func NewWriteSimulation(
	loadSchema func(entity string) map[string]any,
	loadEntity func(entity, id string) map[string]any,
) *WriteSimulation {
	return &WriteSimulation{
		loadSchema: loadSchema,
		loadEntity: loadEntity,
		schemas:    map[string]map[string]any{},
		entities:   map[string]map[string]any{},
	}
}

func (s *WriteSimulation) Write(entity string, data map[string]any) []schema.ValidationError {
	return s.write(entity, data, true)
}

func (s *WriteSimulation) write(entity string, data map[string]any, checkInferredSchema bool) []schema.ValidationError {
	if errs := s.validate(entity, data, checkInferredSchema); len(errs) > 0 {
		return errs
	}

	doc := deepCopyMap(data)
	s.writeSubEntities(entity, doc)
	s.store(entity, doc)

	return []schema.ValidationError{}
}

func (s *WriteSimulation) Update(entity, id string, updates map[string]any) bool {
	existing := s.Read(entity, id)
	if existing == nil {
		return false
	}

	merged := make(map[string]any, len(existing)+len(updates))
	maps.Copy(merged, deepCopyMap(existing))
	maps.Copy(merged, deepCopyMap(updates))
	merged["id"] = id

	s.writeSubEntities(entity, merged)
	s.store(entity, merged)

	return true
}

func (s *WriteSimulation) Delete(entity, id string) {
	s.entities[simulationKey(entity, id)] = nil
}

func (s *WriteSimulation) Read(entity, id string) map[string]any {
	key := simulationKey(entity, id)
	if doc, ok := s.entities[key]; ok {
		return doc
	}

	doc := s.loadEntity(entity, id)
	s.entities[key] = doc

	return doc
}

func (s *WriteSimulation) writeSubEntities(entity string, doc map[string]any) {
	for _, sub := range ExtractSubEntities(entity, doc) {
		subEntity := sub["@entity"].(string)
		delete(sub, "@entity")
		id, _ := sub["id"].(string)
		s.write(subEntity, MergeWithExisting(sub, s.Read(subEntity, id)), false)
	}
}

func (s *WriteSimulation) store(entity string, doc map[string]any) {
	if id, ok := doc["id"].(string); ok && id != "" {
		s.entities[simulationKey(entity, id)] = doc
	}

	if !s.schemaApplies(entity) {
		return
	}

	current := s.schema(entity)
	if globals.GetConfig().Api.Schema.Strict && current != nil && schema.IsManualSchema(entity, current) {
		return
	}

	s.schemas[entity] = schema.MergeAnalyzedSchema(entity, current, doc)
}

func (s *WriteSimulation) validate(entity string, data map[string]any, checkInferredSchema bool) []schema.ValidationError {
	if !s.schemaApplies(entity) {
		return nil
	}

	current := s.schema(entity)
	if current == nil || !checkInferredSchema && !schema.IsManualSchema(entity, current) {
		return nil
	}

	return schema.ValidateEntity(entity, data, current)
}

func (s *WriteSimulation) schema(entity string) map[string]any {
	if current, ok := s.schemas[entity]; ok {
		return current
	}

	current := s.loadSchema(entity)
	s.schemas[entity] = current

	return current
}

func (s *WriteSimulation) schemaApplies(entity string) bool {
	return globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity
}

func simulationKey(entity, id string) string {
	return entity + "\x00" + id
}

func deepCopyMap(in map[string]any) map[string]any {
	out := make(map[string]any, len(in))
	for k, v := range in {
		out[k] = deepCopyValue(v)
	}

	return out
}

func deepCopyValue(v any) any {
	switch t := v.(type) {
	case map[string]any:
		return deepCopyMap(t)
	case []any:
		out := make([]any, len(t))
		for i, item := range t {
			out[i] = deepCopyValue(item)
		}

		return out
	default:
		return t
	}
}

func NestedEntityTypes(data any) []string {
	seen := map[string]bool{}
	var types []string

	var walk func(v any)
	walk = func(v any) {
		switch t := v.(type) {
		case map[string]any:
			if name, ok := t["@entity"].(string); ok && name != "" && !seen[name] {
				seen[name] = true
				types = append(types, name)
			}

			for _, child := range t {
				walk(child)
			}
		case []any:
			for _, child := range t {
				walk(child)
			}
		}
	}

	walk(data)

	return types
}
