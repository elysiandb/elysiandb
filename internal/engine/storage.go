package engine

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/schema"
)

const (
	EngineInternal = "internal"
	EngineMongoDB  = "mongodb"
)

func ExecuteQuery(q query.Query) ([]map[string]any, error) {
	if IsEngineInternal() {
		return api_storage.ExecuteQuery(q)
	}

	if IsEngineMongoDB() {
		return mongodb.ExecuteQuery(q)
	}

	ThrowErrorIfNotValidEngine()

	return nil, nil
}

func FilterFields(data map[string]any, fields []string) map[string]any {
	if IsEngineInternal() {
		return api_storage.FilterFields(data, fields)
	}

	if IsEngineMongoDB() {
		return mongodb.FilterFields(data, fields)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func ApplyIncludes(data []map[string]interface{}, includesParam string) []map[string]interface{} {
	if IsEngineInternal() {
		return api_storage.ApplyIncludes(data, includesParam)
	}

	if IsEngineMongoDB() {
		return mongodb.ApplyIncludes(data, includesParam)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func WriteEntity(entity string, data map[string]interface{}) []schema.ValidationError {
	if IsEngineInternal() {
		return api_storage.WriteEntity(entity, data)
	}

	if IsEngineMongoDB() {
		return mongodb.WriteEntity(entity, data)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func UpdateEntitySchema(entity string, fieldsRaw map[string]interface{}) map[string]interface{} {
	if IsEngineInternal() {
		return api_storage.UpdateEntitySchema(entity, fieldsRaw)
	}

	if IsEngineMongoDB() {
		return mongodb.UpdateEntitySchema(entity, fieldsRaw)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func CreateEntityType(entity string) error {
	if IsEngineInternal() {
		return api_storage.CreateEntityType(entity)
	}

	if IsEngineMongoDB() {
		return mongodb.CreateEntityType(entity)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func DeleteEntityType(entity string) error {
	if IsEngineInternal() {
		return api_storage.DeleteEntityType(entity)
	}

	if IsEngineMongoDB() {
		return mongodb.DeleteEntityType(entity)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func WriteListOfEntities(entity string, list []map[string]interface{}) [][]schema.ValidationError {
	if IsEngineInternal() {
		return api_storage.WriteListOfEntities(entity, list)
	}

	if IsEngineMongoDB() {
		return mongodb.WriteListOfEntities(entity, list)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func AddEntityType(entity string) {
	if IsEngineInternal() {
		api_storage.AddEntityType(entity)
		return
	}

	if IsEngineMongoDB() {
		mongodb.AddEntityType(entity)
		return
	}

	ThrowErrorIfNotValidEngine()
}

func GetEntitySchema(entity string) map[string]interface{} {
	if IsEngineInternal() {
		return api_storage.GetEntitySchema(entity)
	}

	if IsEngineMongoDB() {
		return mongodb.GetEntitySchema(entity)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func EntityTypeExists(entity string) bool {
	if IsEngineInternal() {
		return api_storage.EntityTypeExists(entity)
	}

	if IsEngineMongoDB() {
		return mongodb.EntityTypeExists(entity)
	}

	ThrowErrorIfNotValidEngine()

	return false
}

func ListEntityTypes() []string {
	if IsEngineInternal() {
		return api_storage.ListEntityTypes()
	}

	if IsEngineMongoDB() {
		return mongodb.ListEntityTypes()
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func ListPublicEntityTypes() []string {
	if IsEngineInternal() {
		return api_storage.ListPublicEntityTypes()
	}

	if IsEngineMongoDB() {
		return mongodb.ListPublicEntityTypes()
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func ReadEntityById(entity, id string) map[string]interface{} {
	if IsEngineInternal() {
		return api_storage.ReadEntityById(entity, id)
	}

	if IsEngineMongoDB() {
		return mongodb.ReadEntityById(entity, id)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func ListEntities(
	entity string,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	filters map[string]map[string]string,
	search string,
	includesParam string,
) []map[string]any {
	if IsEngineInternal() {
		return api_storage.ListEntities(entity, limit, offset, sortField, sortAscending, filters, search, includesParam)
	}

	if IsEngineMongoDB() {
		return mongodb.ListEntities(entity, limit, offset, sortField, sortAscending, filters, search, includesParam)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func ApplyFiltersToList(
	entities []map[string]any,
	filters map[string]map[string]string,
) []map[string]any {
	if IsEngineInternal() {
		return api_storage.ApplyFiltersToList(entities, filters)
	}

	if IsEngineMongoDB() {
		return mongodb.ApplyFiltersToList(entities, filters)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func GetListOfIds(entity, sortField string, sortAscending bool) ([]byte, error) {
	if IsEngineInternal() {
		return api_storage.GetListOfIds(entity, sortField, sortAscending)
	}

	if IsEngineMongoDB() {
		return mongodb.GetListOfIds(entity, sortField, sortAscending)
	}

	ThrowErrorIfNotValidEngine()

	return nil, nil
}

func DeleteEntityById(entity, id string) {
	if IsEngineInternal() {
		api_storage.DeleteEntityById(entity, id)
		return
	}

	if IsEngineMongoDB() {
		mongodb.DeleteEntityById(entity, id)
		return
	}

	ThrowErrorIfNotValidEngine()
}

func DeleteAllEntities(entity string) {
	if IsEngineInternal() {
		api_storage.DeleteAllEntities(entity)
		return
	}

	if IsEngineMongoDB() {
		mongodb.DeleteAllEntities(entity)
		return
	}

	ThrowErrorIfNotValidEngine()
}

func DeleteAll() {
	if IsEngineInternal() {
		api_storage.DeleteAll()
		return
	}

	if IsEngineMongoDB() {
		mongodb.DeleteAll()
		return
	}

	ThrowErrorIfNotValidEngine()
}

func UpdateEntityById(entity, id string, updated map[string]interface{}) map[string]interface{} {
	if IsEngineInternal() {
		return api_storage.UpdateEntityById(entity, id, updated)
	}

	if IsEngineMongoDB() {
		return mongodb.UpdateEntityById(entity, id, updated)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func UpdateListOfEntities(entity string, updates []map[string]interface{}) []map[string]interface{} {
	if IsEngineInternal() {
		return api_storage.UpdateListOfEntities(entity, updates)
	}

	if IsEngineMongoDB() {
		return mongodb.UpdateListOfEntities(entity, updates)
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func DumpAll() map[string]interface{} {
	if IsEngineInternal() {
		return api_storage.DumpAll()
	}

	if IsEngineMongoDB() {
		return mongodb.DumpAll()
	}

	ThrowErrorIfNotValidEngine()

	return nil
}

func EntityExists(entity, id string) bool {
	if IsEngineInternal() {
		return api_storage.EntityExists(entity, id)
	}

	if IsEngineMongoDB() {
		return mongodb.EntityExists(entity, id)
	}

	ThrowErrorIfNotValidEngine()

	return false
}

func CountAllEntities() int {
	if IsEngineInternal() {
		return api_storage.CountAllEntities()
	}

	if IsEngineMongoDB() {
		return mongodb.CountAllEntities()
	}

	ThrowErrorIfNotValidEngine()

	return 0
}

func ImportAll(data map[string][]map[string]interface{}) {
	if IsEngineInternal() {
		api_storage.ImportAll(data)
		return
	}

	if IsEngineMongoDB() {
		mongodb.ImportAll(data)
		return
	}

	ThrowErrorIfNotValidEngine()
}

func IsEngineInternal() bool {
	return globals.GetEngine() == EngineInternal
}

func IsEngineMongoDB() bool {
	return globals.GetEngine() == EngineMongoDB
}

func ThrowErrorIfNotValidEngine() {
	engine := globals.GetConfig().Engine.Name
	panic("Invalid storage engine: " + engine + ". Only '" + EngineInternal + "' is supported.")
}
