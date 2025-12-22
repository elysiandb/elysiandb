package mongodb

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

type IncludeSpec struct {
	Path []string
	From string
	As   string
	Tmp  string
}

type RefLoc struct {
	ParentMap map[string]any
	Key       string
	ParentArr []any
	Idx       int
}

func FilterFields(data map[string]any, fields []string) map[string]any {
	if len(fields) == 0 {
		return data
	}

	out := map[string]any{}
	for _, f := range fields {
		if v, ok := data[f]; ok {
			out[f] = v
		}
	}

	return out
}

func ApplyIncludes(data []map[string]interface{}, includesParam string) []map[string]interface{} {
	return data
}

func FindOptions(limit int, offset int, sortField string, sortAscending bool) options.Lister[options.FindOptions] {
	opts := options.Find()

	if limit > 0 {
		opts = opts.SetLimit(int64(limit))
	}
	if offset > 0 {
		opts = opts.SetSkip(int64(offset))
	}
	if sortField != "" {
		dir := 1
		if !sortAscending {
			dir = -1
		}
		opts = opts.SetSort(bson.D{{Key: sortField, Value: dir}})
	}

	return opts
}

func ParseIncludes(includesParam string) (bool, [][]string) {
	includesParam = strings.TrimSpace(includesParam)
	if includesParam == "" {
		return false, nil
	}

	if includesParam == "all" {
		return true, nil
	}

	parts := strings.Split(includesParam, ",")
	out := make([][]string, 0, len(parts))
	for _, p := range parts {
		p = strings.TrimSpace(p)
		if p == "" {
			continue
		}
		chunks := strings.Split(p, ".")
		clean := make([]string, 0, len(chunks))
		for _, c := range chunks {
			c = strings.TrimSpace(c)
			if c != "" {
				clean = append(clean, c)
			}
		}
		if len(clean) > 0 {
			out = append(out, clean)
		}
	}

	return false, out
}

func SingularFallback(s string) string {
	s = strings.TrimSpace(s)
	if s == "" {
		return s
	}
	if strings.HasSuffix(s, "ies") && len(s) > 3 {
		return s[:len(s)-3] + "y"
	}
	if strings.HasSuffix(s, "ses") && len(s) > 3 {
		return s[:len(s)-2]
	}
	if strings.HasSuffix(s, "s") && len(s) > 1 {
		return s[:len(s)-1]
	}
	return s
}

func GetRefId(m map[string]any) (string, bool) {
	if v, ok := m["id"].(string); ok && v != "" {
		return v, true
	}
	if v, ok := m["_id"].(string); ok && v != "" {
		return v, true
	}
	return "", false
}

func IsRefMap(m map[string]any) bool {
	if m == nil {
		return false
	}
	_, ok := m["@entity"].(string)
	if !ok {
		return false
	}
	_, ok = GetRefId(m)
	if !ok {
		return false
	}
	for k := range m {
		if k != "@entity" && k != "id" && k != "_id" {
			return false
		}
	}
	return true
}

func RefEntityFromValue(v any) (string, bool) {
	switch t := v.(type) {
	case map[string]any:
		if ent, ok := t["@entity"].(string); ok && ent != "" {
			return ent, true
		}
	case bson.M:
		if ent, ok := t["@entity"].(string); ok && ent != "" {
			return ent, true
		}
	case bson.D:
		for _, e := range t {
			if e.Key == "@entity" {
				if s, ok := e.Value.(string); ok && s != "" {
					return s, true
				}
			}
		}
	case []any:
		for _, it := range t {
			if ent, ok := RefEntityFromValue(it); ok {
				return ent, true
			}
		}
	case bson.A:
		for _, it := range t {
			if ent, ok := RefEntityFromValue(it); ok {
				return ent, true
			}
		}
	}
	return "", false
}

func BuildSpecsFromSample(entity string, includeAll bool, paths [][]string) []IncludeSpec {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	var sample map[string]any
	_ = globals.MongoDB.Collection(entity).FindOne(ctx, bson.M{}).Decode(&sample)

	if includeAll {
		out := make([]IncludeSpec, 0)
		for k, v := range sample {
			if k == "_id" {
				continue
			}
			ent, ok := RefEntityFromValue(v)
			if !ok || ent == "" {
				continue
			}
			as := k
			out = append(out, IncludeSpec{
				Path: []string{k},
				From: ent,
				As:   as,
				Tmp:  "__ely_ref_" + as,
			})
		}
		return out
	}

	out := make([]IncludeSpec, 0)
	for _, p := range paths {
		if len(p) == 0 {
			continue
		}
		if len(p) != 1 {
			continue
		}
		field := p[0]
		from := ""
		if v, ok := sample[field]; ok {
			if ent, ok := RefEntityFromValue(v); ok {
				from = ent
			}
		}
		if from == "" {
			from = SingularFallback(field)
		}
		as := field
		out = append(out, IncludeSpec{
			Path: []string{field},
			From: from,
			As:   as,
			Tmp:  "__ely_ref_" + as,
		})
	}

	return out
}

func AddIncludeEntityTags(items []map[string]any, specs []IncludeSpec) []map[string]any {
	if len(specs) == 0 {
		return items
	}

	for _, it := range items {
		for _, sp := range specs {
			if arr, ok := it[sp.As].([]any); ok {
				for _, v := range arr {
					if m, ok := v.(map[string]any); ok {
						m["@entity"] = sp.From
					}
				}
			}
		}
	}

	return items
}

func CollectRefsAtPath(root any, path []string, out *[]struct {
	Loc    RefLoc
	Entity string
	Id     string
}) {
	if len(path) == 0 {
		return
	}

	switch t := root.(type) {
	case map[string]any:
		seg := path[0]
		v, ok := t[seg]
		if !ok {
			return
		}
		if len(path) == 1 {
			switch vv := v.(type) {
			case map[string]any:
				if IsRefMap(vv) {
					ent, _ := vv["@entity"].(string)
					id, ok := GetRefId(vv)
					if ok && ent != "" {
						*out = append(*out, struct {
							Loc    RefLoc
							Entity string
							Id     string
						}{
							Loc:    RefLoc{ParentMap: t, Key: seg},
							Entity: ent,
							Id:     id,
						})
					}
				}
			case []any:
				for i, it := range vv {
					if m, ok := it.(map[string]any); ok && IsRefMap(m) {
						ent, _ := m["@entity"].(string)
						id, ok := GetRefId(m)
						if ok && ent != "" {
							*out = append(*out, struct {
								Loc    RefLoc
								Entity string
								Id     string
							}{
								Loc:    RefLoc{ParentArr: vv, Idx: i},
								Entity: ent,
								Id:     id,
							})
						}
					}
				}
			}
			return
		}

		switch vv := v.(type) {
		case map[string]any:
			CollectRefsAtPath(vv, path[1:], out)
		case []any:
			for _, it := range vv {
				CollectRefsAtPath(it, path[1:], out)
			}
		}
	case []any:
		for _, it := range t {
			CollectRefsAtPath(it, path, out)
		}
	}
}

func CollectAllRefMaps(root any, out *[]struct {
	Loc    RefLoc
	Entity string
	Id     string
}) {
	switch t := root.(type) {
	case map[string]any:
		for k, v := range t {
			if m, ok := v.(map[string]any); ok && IsRefMap(m) {
				ent, _ := m["@entity"].(string)
				id, ok := GetRefId(m)
				if ok && ent != "" {
					*out = append(*out, struct {
						Loc    RefLoc
						Entity string
						Id     string
					}{
						Loc:    RefLoc{ParentMap: t, Key: k},
						Entity: ent,
						Id:     id,
					})
				}
				continue
			}
			if arr, ok := v.([]any); ok {
				for i, it := range arr {
					if m, ok := it.(map[string]any); ok && IsRefMap(m) {
						ent, _ := m["@entity"].(string)
						id, ok := GetRefId(m)
						if ok && ent != "" {
							*out = append(*out, struct {
								Loc    RefLoc
								Entity string
								Id     string
							}{
								Loc:    RefLoc{ParentArr: arr, Idx: i},
								Entity: ent,
								Id:     id,
							})
						}
					} else {
						CollectAllRefMaps(it, out)
					}
				}
				continue
			}
			CollectAllRefMaps(v, out)
		}
	case []any:
		for _, it := range t {
			CollectAllRefMaps(it, out)
		}
	}
}

func LoadDocsByIds(entity string, ids []string) map[string]map[string]any {
	if len(ids) == 0 {
		return map[string]map[string]any{}
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	q := bson.M{"_id": bson.M{"$in": ids}}
	cur, err := globals.MongoDB.Collection(entity).Find(ctx, q)
	if err != nil || cur == nil {
		return map[string]map[string]any{}
	}
	defer cur.Close(ctx)

	out := map[string]map[string]any{}
	for cur.Next(ctx) {
		var raw map[string]any
		if cur.Decode(&raw) == nil {
			n := NormalizeMongoDocument(raw)
			if id, ok := n["id"].(string); ok && id != "" {
				out[id] = n
			}
		}
	}

	return out
}

func ApplyLoadedRefs(refs []struct {
	Loc    RefLoc
	Entity string
	Id     string
}, loaded map[string]map[string]map[string]any) {
	for _, r := range refs {
		entMap, ok := loaded[r.Entity]
		if !ok {
			continue
		}
		doc, ok := entMap[r.Id]
		if !ok || doc == nil {
			continue
		}
		doc["@entity"] = r.Entity

		if r.Loc.ParentMap != nil {
			r.Loc.ParentMap[r.Loc.Key] = doc
		} else if r.Loc.ParentArr != nil && r.Loc.Idx >= 0 && r.Loc.Idx < len(r.Loc.ParentArr) {
			r.Loc.ParentArr[r.Loc.Idx] = doc
		}
	}
}

func ResolveIncludesPaths(items []map[string]any, paths [][]string) {
	if len(paths) == 0 || len(items) == 0 {
		return
	}

	for _, p := range paths {
		refs := make([]struct {
			Loc    RefLoc
			Entity string
			Id     string
		}, 0)

		for _, it := range items {
			CollectRefsAtPath(it, p, &refs)
		}

		if len(refs) == 0 {
			continue
		}

		byEntity := map[string]map[string]struct{}{}
		for _, r := range refs {
			m, ok := byEntity[r.Entity]
			if !ok {
				m = map[string]struct{}{}
				byEntity[r.Entity] = m
			}
			m[r.Id] = struct{}{}
		}

		loaded := map[string]map[string]map[string]any{}
		for ent, set := range byEntity {
			ids := make([]string, 0, len(set))
			for id := range set {
				ids = append(ids, id)
			}
			loaded[ent] = LoadDocsByIds(ent, ids)
		}

		ApplyLoadedRefs(refs, loaded)
	}
}

func ResolveIncludesAllRecursive(items []map[string]any, maxDepth int) {
	if len(items) == 0 || maxDepth <= 0 {
		return
	}

	for depth := 0; depth < maxDepth; depth++ {
		refs := make([]struct {
			Loc    RefLoc
			Entity string
			Id     string
		}, 0)

		for _, it := range items {
			CollectAllRefMaps(it, &refs)
		}

		if len(refs) == 0 {
			return
		}

		byEntity := map[string]map[string]struct{}{}
		for _, r := range refs {
			m, ok := byEntity[r.Entity]
			if !ok {
				m = map[string]struct{}{}
				byEntity[r.Entity] = m
			}
			m[r.Id] = struct{}{}
		}

		loaded := map[string]map[string]map[string]any{}
		for ent, set := range byEntity {
			ids := make([]string, 0, len(set))
			for id := range set {
				ids = append(ids, id)
			}
			loaded[ent] = LoadDocsByIds(ent, ids)
		}

		ApplyLoadedRefs(refs, loaded)
	}
}

func WriteEntity(entity string, data map[string]interface{}) []schema.ValidationError {
	schemaData := GetEntitySchema(entity)
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		errors := schema.ValidateEntity(entity, data, schemaData)
		if len(errors) > 0 {
			return errors
		}
	}

	if !EntityTypeExists(entity) {
		AddEntityType(entity)
	}

	if _, ok := data["id"].(string); !ok || data["id"] == "" {
		data["id"] = uuid.New().String()
	}

	subs := api_storage.ExtractSubEntities(entity, data)
	for _, sub := range subs {
		subEntity := sub["@entity"].(string)
		delete(sub, "@entity")
		WriteEntity(subEntity, sub)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	doc := ToMongoDocument(data)

	_, err := globals.MongoDB.Collection(entity).ReplaceOne(
		ctx,
		bson.M{"_id": doc["_id"]},
		doc,
		options.Replace().SetUpsert(true),
	)

	if err != nil {
		return []schema.ValidationError{{Message: err.Error()}}
	}

	if !schema.IsManualSchema(entity, schemaData) || !globals.GetConfig().Api.Schema.Strict {
		UpdateSchemaIfNeeded(entity, data)
	}

	return []schema.ValidationError{}
}

func UpdateSchemaIfNeeded(entity string, data map[string]interface{}) {
	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		analyzed := schema.AnalyzeEntitySchema(entity, data)
		WriteEntity(schema.SchemaEntity, analyzed)
	}
}

func UpdateEntitySchema(entity string, fieldsRaw map[string]interface{}) map[string]interface{} {
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
	ctx := context.Background()

	if err := globals.MongoDB.Collection(entity).Drop(ctx); err != nil {
		return err
	}

	return api_storage.DeleteEntityType(entity)
}

func WriteListOfEntities(entity string, list []map[string]interface{}) [][]schema.ValidationError {
	out := make([][]schema.ValidationError, 0, len(list))
	for _, d := range list {
		out = append(out, WriteEntity(entity, d))
	}
	return out
}

func AddEntityType(entity string) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	names, _ := globals.MongoDB.ListCollectionNames(ctx, bson.M{"name": entity})
	if len(names) == 0 {
		_ = globals.MongoDB.CreateCollection(ctx, entity)
	}

	api_storage.AddEntityType(entity)
}

func GetEntitySchema(entity string) map[string]interface{} {
	return ReadEntityById(schema.SchemaEntity, entity)
}

func EntityTypeExists(entity string) bool {
	return api_storage.EntityTypeExists(entity)
}

func ListEntityTypes() []string {
	return api_storage.ListEntityTypes()
}

func ListPublicEntityTypes() []string {
	return api_storage.ListPublicEntityTypes()
}

func ReadEntityById(entity, id string) map[string]any {
	ctx := context.Background()

	raw := map[string]any{}
	if globals.MongoDB.
		Collection(entity).
		FindOne(ctx, bson.M{"_id": id}).
		Decode(&raw) != nil {
		return nil
	}

	return NormalizeMongoDocument(raw)
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
	ctx := context.Background()
	q := BuildMongoFilters(filters)

	includeAll, paths := ParseIncludes(includesParam)

	rootSpecs := BuildSpecsFromSample(entity, includeAll, paths)

	leafPaths := make([][]string, 0)
	for _, p := range paths {
		if len(p) > 1 {
			leafPaths = append(leafPaths, p)
		}
	}

	if len(rootSpecs) == 0 {
		cur, err := globals.MongoDB.
			Collection(entity).
			Find(ctx, q, FindOptions(limit, offset, sortField, sortAscending))
		if err != nil || cur == nil {
			return []map[string]any{}
		}
		defer cur.Close(ctx)

		out := make([]map[string]any, 0)
		for cur.Next(ctx) {
			var raw map[string]any
			if cur.Decode(&raw) == nil {
				out = append(out, NormalizeMongoDocument(raw))
			}
		}

		if includeAll {
			ResolveIncludesAllRecursive(out, 8)
		} else if len(leafPaths) > 0 {
			ResolveIncludesPaths(out, leafPaths)
		}

		return out
	}

	pipeline := make([]bson.D, 0, 10+len(rootSpecs)*3)
	pipeline = append(pipeline, bson.D{{Key: "$match", Value: q}})

	if sortField != "" {
		dir := 1
		if !sortAscending {
			dir = -1
		}
		pipeline = append(pipeline, bson.D{{Key: "$sort", Value: bson.D{{Key: sortField, Value: dir}}}})
	}
	if offset > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$skip", Value: int64(offset)}})
	}
	if limit > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$limit", Value: int64(limit)}})
	}

	addFields := bson.D{}
	for _, sp := range rootSpecs {
		field := "$" + sp.Path[0]
		addFields = append(addFields, bson.E{
			Key: sp.Tmp,
			Value: bson.D{
				{Key: "$cond", Value: bson.D{
					{Key: "if", Value: bson.D{{Key: "$isArray", Value: field}}},
					{Key: "then", Value: bson.D{{Key: "$map", Value: bson.D{
						{Key: "input", Value: field},
						{Key: "as", Value: "r"},
						{Key: "in", Value: bson.D{{Key: "$ifNull", Value: bson.A{
							"$$r.id",
							"$$r._id",
						}}}},
					}}}},
					{Key: "else", Value: bson.D{{Key: "$ifNull", Value: bson.A{
						field + ".id",
						"$" + sp.Path[0] + "Id",
					}}}},
				}},
			},
		})
	}
	if len(addFields) > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$addFields", Value: addFields}})
	}

	project := bson.D{}
	for _, sp := range rootSpecs {
		project = append(project, bson.E{Key: sp.Tmp, Value: 0})
	}

	for _, sp := range rootSpecs {
		pipeline = append(pipeline,
			bson.D{{Key: "$lookup", Value: bson.M{
				"from":         sp.From,
				"localField":   sp.Tmp,
				"foreignField": "_id",
				"as":           sp.As,
			}}},
		)
	}

	if len(project) > 0 {
		pipeline = append(pipeline, bson.D{{Key: "$project", Value: project}})
	}

	cur, err := globals.MongoDB.Collection(entity).Aggregate(ctx, pipeline)
	if err != nil || cur == nil {
		return []map[string]any{}
	}
	defer cur.Close(ctx)

	out := make([]map[string]any, 0)
	for cur.Next(ctx) {
		var raw map[string]any
		if cur.Decode(&raw) == nil {
			out = append(out, NormalizeMongoDocument(raw))
		}
	}

	out = AddIncludeEntityTags(out, rootSpecs)

	if includeAll {
		ResolveIncludesAllRecursive(out, 8)
	} else if len(leafPaths) > 0 {
		ResolveIncludesPaths(out, leafPaths)
	}

	return out
}

func ApplyFiltersToList(entities []map[string]any, filters map[string]map[string]string) []map[string]any {
	return entities
}

func GetListOfIds(entity, sortField string, sortAscending bool) ([]byte, error) {
	return api_storage.GetListOfIds(entity, sortField, sortAscending)
}

func DeleteEntityById(entity, id string) {
	ctx := context.Background()
	globals.MongoDB.Collection(entity).DeleteOne(ctx, bson.M{"_id": id})
}

func DeleteAllEntities(entity string) {
	ctx := context.Background()
	globals.MongoDB.Collection(entity).DeleteMany(ctx, bson.M{})
	DeleteEntityType(entity)
}

func DeleteAll() {
	for _, e := range ListEntityTypes() {
		DeleteAllEntities(e)
	}
	api_storage.DeleteAll()
}

func UpdateEntityById(entity, id string, updated map[string]interface{}) map[string]interface{} {
	ctx := context.Background()
	update := ToMongoDocument(updated)
	globals.MongoDB.Collection(entity).UpdateOne(ctx, bson.M{"_id": id}, bson.M{"$set": update})
	return ReadEntityById(entity, id)
}

func UpdateListOfEntities(entity string, updates []map[string]interface{}) []map[string]interface{} {
	out := make([]map[string]interface{}, 0, len(updates))
	for _, u := range updates {
		if id, ok := u["id"].(string); ok {
			out = append(out, UpdateEntityById(entity, id, u))
		}
	}
	return out
}

func DumpAll() map[string]interface{} {
	out := map[string]interface{}{}
	for _, e := range ListEntityTypes() {
		out[e] = ListEntities(e, 0, 0, "", true, nil, "", "")
	}
	return out
}

func EntityExists(entity, id string) bool {
	ctx := context.Background()
	n, _ := globals.MongoDB.Collection(entity).CountDocuments(ctx, bson.M{"_id": id})
	return n > 0
}

func CountAllEntities() int {
	total := 0
	for _, e := range ListEntityTypes() {
		ctx := context.Background()
		n, _ := globals.MongoDB.Collection(e).CountDocuments(ctx, bson.M{})
		total += int(n)
	}
	return total
}

func ImportAll(data map[string][]map[string]interface{}) {
	for e, list := range data {
		for _, d := range list {
			WriteEntity(e, d)
		}
	}
}
