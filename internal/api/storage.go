package api_storage

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"reflect"
	"slices"
	"strings"

	"github.com/google/uuid"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/storage"
)

const CoreEntityTypePrefix = "_elysiandb_core_"

var (
	errValidation = errors.New("validation failed")
	errNotOpen    = errors.New("storage is not open")
	apiLog        = log.For("api")
)

func db() *datastore.Engine {
	return storage.Engine()
}

func update(fn func(tx *datastore.Tx) error) error {
	e := db()
	if e == nil {
		return errNotOpen
	}

	return e.Update(fn)
}

func logWriteError(op, entity string, err error) {
	if err != nil && !errors.Is(err, errValidation) {
		apiLog.Error("write failed", "op", op, "entity", entity, "err", err)
	}
}

func replaceContents(dst, src map[string]any) {
	for k := range dst {
		delete(dst, k)
	}

	maps.Copy(dst, src)
}

func WriteEntity(entity string, data map[string]any) []schema.ValidationError {
	var errs []schema.ValidationError
	var result map[string]any

	err := update(func(tx *datastore.Tx) error {
		attempt := storage.DeepCopyDocument(data)
		errs = writeEntity(tx, entity, attempt, true)
		if len(errs) > 0 {
			return errValidation
		}

		result = attempt

		return nil
	})

	if errors.Is(err, errValidation) {
		return errs
	}

	logWriteError("write", entity, err)
	if result != nil {
		replaceContents(data, result)
	}

	return []schema.ValidationError{}
}

func schemaFor(tx *datastore.Tx, entity string) map[string]any {
	if tx == nil {
		return GetEntitySchema(entity)
	}

	return tx.ReadShared(schema.SchemaEntity, entity)
}

func writeEntity(tx *datastore.Tx, entity string, data map[string]any, checkInferredSchema bool) []schema.ValidationError {
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		current := schemaFor(tx, entity)
		if current != nil && (checkInferredSchema || schema.IsManualSchema(entity, current)) {
			if errors := schema.ValidateEntity(entity, data, current); len(errors) > 0 {
				return errors
			}
		}
	}

	if _, ok := data["id"].(string); !ok || data["id"] == "" {
		data["id"] = uuid.New().String()
	}

	writeSubEntities(tx, entity, data)

	persistEntity(tx, entity, data)
	updateSchemaIfNeeded(tx, entity, data)

	return []schema.ValidationError{}
}

func writeSubEntities(tx *datastore.Tx, entity string, data map[string]any) {
	subs := ExtractSubEntities(entity, data)
	for _, sub := range subs {
		subEntity := sub["@entity"].(string)
		delete(sub, "@entity")
		id, _ := sub["id"].(string)
		writeEntity(tx, subEntity, MergeWithExisting(sub, tx.Read(subEntity, id)), false)
	}
}

func UpdateEntitySchema(entity string, fieldsRaw map[string]any) map[string]any {
	fields := schema.MapToFields(fieldsRaw)

	entitySchema := schema.Entity{
		ID:     entity,
		Fields: fields,
	}

	storable := schema.SchemaEntityToStorableStructure(entitySchema)
	storable["_manual"] = true

	WriteEntity(schema.SchemaEntity, storable)

	return storable
}

func CreateEntityType(entity string) error {
	if EntityTypeExists(entity) {
		return fmt.Errorf("entity type '%s' already exists", entity)
	}

	AddEntityType(entity)

	return nil
}

func DeleteEntityType(entity string) error {
	if !EntityTypeExists(entity) {
		return fmt.Errorf("entity type '%s' does not exist", entity)
	}

	DeleteAllEntities(entity)

	return nil
}

func persistEntity(tx *datastore.Tx, entity string, data map[string]any) {
	id, _ := data["id"].(string)
	if err := tx.Put(entity, id, data); err != nil {
		apiLog.Error("cannot encode a document", "entity", entity, "id", id, "err", err)
		return
	}

	tx.EnsureType(entity)
}

func updateSchemaIfNeeded(tx *datastore.Tx, entity string, data map[string]any) {
	if !globals.GetConfig().Api.Schema.Enabled || entity == schema.SchemaEntity {
		return
	}

	current := tx.ReadShared(schema.SchemaEntity, entity)
	if globals.GetConfig().Api.Schema.Strict && current != nil && schema.IsManualSchema(entity, current) {
		return
	}

	merged := schema.MergeAnalyzedSchema(entity, current, data)
	if current != nil && reflect.DeepEqual(merged, current) {
		return
	}

	if current != nil {
		encoded, err := json.Marshal(merged)
		if stored, ok := tx.ReadBytes(schema.SchemaEntity, entity); err == nil && ok && bytes.Equal(encoded, stored) {
			return
		}
	}

	writeEntity(tx, schema.SchemaEntity, merged, true)
}

func WriteListOfEntities(entity string, list []map[string]any) [][]schema.ValidationError {
	var errors [][]schema.ValidationError
	var results []map[string]any

	err := update(func(tx *datastore.Tx) error {
		errors = [][]schema.ValidationError{}
		results = make([]map[string]any, len(list))
		for i, data := range list {
			attempt := storage.DeepCopyDocument(data)
			errs := writeEntity(tx, entity, attempt, true)
			errors = append(errors, errs)
			if len(errs) == 0 {
				results[i] = attempt
			}
		}

		return nil
	})

	logWriteError("write_list", entity, err)
	for i, result := range results {
		if result != nil {
			replaceContents(list[i], result)
		}
	}

	return errors
}

func AddEntityType(entity string) {
	if EntityTypeExists(entity) {
		return
	}

	logWriteError("add_type", entity, update(func(tx *datastore.Tx) error {
		tx.EnsureType(entity)
		return nil
	}))
}

func GetEntitySchema(entity string) map[string]any {
	schemaData := ReadEntityById(schema.SchemaEntity, entity)
	if schemaData == nil {
		return nil
	}

	return schemaData
}

func EntityTypeExists(entity string) bool {
	e := db()
	return e != nil && e.TypeExists(entity)
}

func ListEntityTypes() []string {
	e := db()
	if e == nil {
		return nil
	}

	return e.Types()
}

func IsCoreEntityType(entity string) bool {
	return strings.HasPrefix(entity, CoreEntityTypePrefix)
}

func ListPublicEntityTypes() []string {
	allEntityTypes := ListEntityTypes()

	publicEntityTypes := make([]string, 0, len(allEntityTypes))
	for _, entityType := range allEntityTypes {
		if !IsCoreEntityType(entityType) {
			publicEntityTypes = append(publicEntityTypes, entityType)
		}
	}

	return publicEntityTypes
}

func ReadEntityById(entity, id string) map[string]any {
	e := db()
	if e == nil {
		return nil
	}

	b, ok := e.Read(entity, id)
	if !ok {
		return nil
	}

	return decodeMap(b)
}

func ReadEntityBytesById(entity, id string) ([]byte, bool) {
	e := db()
	if e == nil {
		return nil, false
	}

	return e.Read(entity, id)
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
	out, _ := ListEntitiesGuarded(entity, limit, offset, sortField, sortAscending, filters, search, includesParam, nil)

	return out
}

func ListEntitiesGuarded(
	entity string,
	limit int,
	offset int,
	sortField string,
	sortAscending bool,
	filters map[string]map[string]string,
	search string,
	includesParam string,
	guard *Guard,
) ([]map[string]any, error) {
	includes := MergeIncludes(includesParam, ExtractAutoIncludes(filters))
	res := Plan(ListQuery{
		Entity:    entity,
		Limit:     limit,
		Offset:    offset,
		SortField: sortField,
		SortAsc:   sortAscending,
		Filters:   filters,
		Search:    search,
		Includes:  includes,
		Guard:     guard,
	})

	if res.Err != nil {
		return []map[string]any{}, res.Err
	}

	out := res.Decode()
	if includes != "" && len(out) > 0 {
		out = applyIncludesWith(out, includes, res.reader())
	}

	return out, guard.Err()
}

func ApplyFiltersToList(
	entities []map[string]any,
	filters map[string]map[string]string,
) []map[string]any {
	if len(filters) == 0 {
		return entities
	}

	filtered := make([]map[string]any, 0, len(entities))
	for _, e := range entities {
		if FiltersMatchEntity(e, filters) {
			filtered = append(filtered, e)
		}
	}

	return filtered
}

func Paginate(entities []map[string]any, offset, limit int) []map[string]any {
	return applyOffsetLimit(entities, offset, limit)
}

func applyOffsetLimit[T any](in []T, offset, limit int) []T {
	start := offset
	if start < 0 {
		start = 0
	}

	if start > len(in) {
		start = len(in)
	}

	end := len(in)
	if limit > 0 && limit < end-start {
		end = start + limit
	}

	return in[start:end]
}

func GetListOfIds(entity, sortField string, sortAscending bool) ([]byte, error) {
	res := Plan(ListQuery{Entity: entity, SortField: sortField, SortAsc: sortAscending})
	ids := res.IDs()
	if len(ids) == 0 {
		if sortField == "" {
			return nil, fmt.Errorf("key not found: %s", globals.ApiEntityIndexIdKey(entity))
		}

		return []byte{}, nil
	}

	return []byte(strings.Join(ids, "\n")), nil
}

func DeleteEntityById(entity, id string) {
	logWriteError("delete", entity, update(func(tx *datastore.Tx) error {
		tx.Delete(entity, id)
		return nil
	}))
}

func deleteAllEntities(tx *datastore.Tx, entity string) {
	if !schema.IsManualSchema(entity, tx.Read(schema.SchemaEntity, entity)) {
		tx.Delete(schema.SchemaEntity, entity)
	}

	tx.ResetEntity(entity)
	tx.DropType(entity)
}

func DeleteAllEntities(entity string) {
	logWriteError("delete_all", entity, update(func(tx *datastore.Tx) error {
		deleteAllEntities(tx, entity)
		return nil
	}))
}

func DeleteAll() {
	logWriteError("delete_all", "*", update(func(tx *datastore.Tx) error {
		for _, entity := range ListEntityTypes() {
			deleteAllEntities(tx, entity)
		}
		return nil
	}))
}

func UpdateEntityById(entity, id string, updated map[string]any) map[string]any {
	var result map[string]any
	err := update(func(tx *datastore.Tx) error {
		result = updateEntityById(tx, entity, id, updated)
		return nil
	})

	logWriteError("update", entity, err)

	return result
}

func updateEntityById(tx *datastore.Tx, entity, id string, updated map[string]any) map[string]any {
	existing := tx.Read(entity, id)
	if existing == nil {
		return nil
	}

	maps.Copy(existing, storage.DeepCopyDocument(updated))
	existing["id"] = id

	writeSubEntities(tx, entity, existing)

	persistEntity(tx, entity, existing)
	updateSchemaIfNeeded(tx, entity, existing)

	return existing
}

func UpdateListOfEntities(entity string, updates []map[string]any) []map[string]any {
	var results []map[string]any
	err := update(func(tx *datastore.Tx) error {
		results = make([]map[string]any, 0, len(updates))
		for _, upd := range updates {
			id, ok := upd["id"].(string)
			if !ok || id == "" {
				continue
			}

			if res := updateEntityById(tx, entity, id, upd); res != nil {
				results = append(results, res)
			}
		}

		return nil
	})

	logWriteError("update_list", entity, err)

	return results
}

func DumpAll() map[string]any {
	entities := ListEntityTypes()

	result := make(map[string]any)
	for _, entity := range entities {
		if entity == schema.SchemaEntity {
			continue
		}

		result[entity] = ListEntities(entity, 0, 0, "", true, nil, "", "")
	}

	return result
}

func EntityExists(entity, id string) bool {
	e := db()
	if e == nil {
		return false
	}

	_, ok := e.Read(entity, id)

	return ok
}

func CountEntities(entity string) int {
	e := db()
	if e == nil {
		return 0
	}

	return e.Count(entity)
}

func CountEntitiesPerType() map[string]int {
	counts := map[string]int{}
	for _, entity := range ListPublicEntityTypes() {
		counts[entity] = CountEntities(entity)
	}

	return counts
}

func CountAllEntities() int {
	counter := 0
	for _, entity := range ListEntityTypes() {
		if entity == schema.SchemaEntity {
			continue
		}

		counter += CountEntities(entity)
	}

	return counter
}

type ImportRejection struct {
	Entity string                   `json:"entity"`
	Index  int                      `json:"index"`
	ID     string                   `json:"id,omitempty"`
	Errors []schema.ValidationError `json:"errors"`
}

func ValidateImport(data map[string][]map[string]any, schemaFor func(entity string) map[string]any) []ImportRejection {
	rejections := []ImportRejection{}
	if !globals.GetConfig().Api.Schema.Enabled {
		return rejections
	}

	for _, entity := range slices.Sorted(maps.Keys(data)) {
		if entity == schema.SchemaEntity {
			continue
		}

		schemaData := schemaFor(entity)
		if !schema.IsManualSchema(entity, schemaData) {
			continue
		}

		for index, item := range data[entity] {
			if errs := schema.ValidateEntity(entity, item, schemaData); len(errs) > 0 {
				rejections = append(rejections, NewImportRejection(entity, index, item, errs))
			}
		}
	}

	return rejections
}

func NewImportRejection(entity string, index int, item map[string]any, errs []schema.ValidationError) ImportRejection {
	id, _ := item["id"].(string)

	return ImportRejection{
		Entity: entity,
		Index:  index,
		ID:     id,
		Errors: errs,
	}
}

const importBatch = 1000

func ImportAll(data map[string][]map[string]any) []ImportRejection {
	rejections := []ImportRejection{}
	for _, entity := range slices.Sorted(maps.Keys(data)) {
		DeleteEntityDocuments(entity)

		items := data[entity]
		for start := 0; start < len(items); start += importBatch {
			end := min(start+importBatch, len(items))
			var batch []ImportRejection
			err := update(func(tx *datastore.Tx) error {
				batch = nil
				for index := start; index < end; index++ {
					attempt := storage.DeepCopyDocument(items[index])
					if errs := writeEntity(tx, entity, attempt, false); len(errs) > 0 {
						batch = append(batch, NewImportRejection(entity, index, items[index], errs))
						continue
					}
					replaceContents(items[index], attempt)
				}
				return nil
			})
			logWriteError("import", entity, err)
			rejections = append(rejections, batch...)
		}

		log.Debug("entity type imported", "component", "api", "entity", entity, "documents", len(data[entity]))
	}

	return rejections
}

func DeleteEntityDocuments(entity string) {
	logWriteError("delete_documents", entity, update(func(tx *datastore.Tx) error {
		tx.ResetEntity(entity)
		return nil
	}))
}

type BatchOperation struct {
	Kind   string
	Entity string
	ID     string
	Data   map[string]any
}

var (
	ErrBatchValidation = errors.New("validation error")
	ErrBatchUpdate     = errors.New("update failed")
)

func CommitBatch(ops []BatchOperation) error {
	return update(func(tx *datastore.Tx) error {
		for _, op := range ops {
			switch op.Kind {
			case "write":
				if errs := writeEntity(tx, op.Entity, storage.DeepCopyDocument(op.Data), true); len(errs) > 0 {
					return ErrBatchValidation
				}
			case "update":
				if updateEntityById(tx, op.Entity, op.ID, op.Data) == nil {
					return ErrBatchUpdate
				}
			case "delete":
				tx.Delete(op.Entity, op.ID)
			}
		}

		return nil
	})
}

func WipeAll() error {
	return update(func(tx *datastore.Tx) error {
		tx.ClearAll()
		return nil
	})
}
