package engine

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/storage"
)

func FilterFields(data map[string]any, fields []string) map[string]any {
	return api_storage.FilterFields(data, fields)
}

func ApplyIncludes(data []map[string]interface{}, includesParam string) []map[string]interface{} {
	return api_storage.ApplyIncludes(data, includesParam)
}

func WriteEntity(entity string, data map[string]interface{}) []schema.ValidationError {
	return api_storage.WriteEntity(entity, data)
}

func UpdateEntitySchema(entity string, fieldsRaw map[string]interface{}) map[string]interface{} {
	return api_storage.UpdateEntitySchema(entity, fieldsRaw)
}

func CreateEntityType(entity string) error {
	return api_storage.CreateEntityType(entity)
}

func DeleteEntityType(entity string) error {
	return api_storage.DeleteEntityType(entity)
}

func WriteListOfEntities(entity string, list []map[string]interface{}) [][]schema.ValidationError {
	return api_storage.WriteListOfEntities(entity, list)
}

func AddEntityType(entity string) {
	api_storage.AddEntityType(entity)
}

func GetEntitySchema(entity string) map[string]interface{} {
	return api_storage.GetEntitySchema(entity)
}

func EntityTypeExists(entity string) bool {
	return api_storage.EntityTypeExists(entity)
}

func ListEntityTypes() []string {
	return api_storage.ListEntityTypes()
}

func IsCoreEntityType(entity string) bool {
	return api_storage.IsCoreEntityType(entity)
}

func ListPublicEntityTypes() []string {
	return api_storage.ListPublicEntityTypes()
}

func ReadEntityById(entity, id string) map[string]interface{} {
	return api_storage.ReadEntityById(entity, id)
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
	return api_storage.ListEntities(entity, limit, offset, sortField, sortAscending, filters, search, includesParam)
}

func ApplyFiltersToList(
	entities []map[string]any,
	filters map[string]map[string]string,
) []map[string]any {
	return api_storage.ApplyFiltersToList(entities, filters)
}

func GetListOfIds(entity, sortField string, sortAscending bool) ([]byte, error) {
	return api_storage.GetListOfIds(entity, sortField, sortAscending)
}

func DeleteEntityById(entity, id string) {
	api_storage.DeleteEntityById(entity, id)
}

func DeleteAllEntities(entity string) {
	api_storage.DeleteAllEntities(entity)
}

func DeleteAll() {
	api_storage.DeleteAll()
}

func UpdateEntityById(entity, id string, updated map[string]interface{}) map[string]interface{} {
	return api_storage.UpdateEntityById(entity, id, updated)
}

func UpdateListOfEntities(entity string, updates []map[string]interface{}) []map[string]interface{} {
	return api_storage.UpdateListOfEntities(entity, updates)
}

func EntityExists(entity, id string) bool {
	return api_storage.EntityExists(entity, id)
}

func CountAllEntities() int {
	return api_storage.CountAllEntities()
}

func CountEntitiesPerType() map[string]int {
	return api_storage.CountEntitiesPerType()
}

func ValidateImport(data map[string][]map[string]interface{}) []api_storage.ImportRejection {
	return api_storage.ValidateImport(data, GetEntitySchema)
}

func ImportAll(data map[string][]map[string]interface{}) []api_storage.ImportRejection {
	return api_storage.ImportAll(data)
}

func IterateEntities(entity string, fn func(data map[string]any) error) error {
	return api_storage.IterateEntities(entity, fn)
}

func RestoreEntities(entity string, next func() (map[string]any, error)) (int, error) {
	return api_storage.RestoreEntities(entity, next)
}

func WipeAll() error {
	storage.ClearAll()

	return nil
}
