package mongodb_test

import (
	"context"
	"encoding/json"
	"fmt"
	"math/rand"
	"slices"
	"sort"
	"strings"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/test/mongotest"
	"go.mongodb.org/mongo-driver/v2/bson"
)

type engineUnderTest struct {
	name          string
	write         func(entity string, data map[string]any)
	list          func(entity string, limit, offset int, sortField string, asc bool, filters map[string]map[string]string, search, includes string) []map[string]any
	query         func(q query.Query) ([]map[string]any, error)
	update        func(entity, id string, data map[string]any) map[string]any
	remove        func(entity, id string)
	dump          func() map[string]any
	types         func() []string
	count         func() int
	read          func(entity, id string) map[string]any
	applyIncludes func(data []map[string]any, includes string) []map[string]any
	migrate       func(q []api_storage.MigrationQuery) error
}

func setupEngines(t *testing.T) (engineUnderTest, engineUnderTest) {
	t.Helper()

	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{Folder: t.TempDir(), Shards: 8},
	})
	storage.LoadDB()
	storage.LoadJsonDB()
	mongotest.Use(t)

	internal := engineUnderTest{
		name:  "internal",
		write: func(e string, d map[string]any) { api_storage.WriteEntity(e, d) },
		list:  api_storage.ListEntities,
		query: api_storage.ExecuteQuery,
		update: func(e, id string, d map[string]any) map[string]any {
			return api_storage.UpdateEntityById(e, id, d)
		},
		remove: api_storage.DeleteEntityById,
		dump:   api_storage.DumpAll,
		types:  api_storage.ListEntityTypes,
		count:  api_storage.CountAllEntities,
		read:   api_storage.ReadEntityById,
		applyIncludes: func(d []map[string]any, inc string) []map[string]any {
			return api_storage.ApplyIncludes(d, inc, api_storage.ReadEntityById)
		},
		migrate: api_storage.ExecuteMigrations,
	}

	mongo := engineUnderTest{
		name:          "mongodb",
		write:         func(e string, d map[string]any) { mongodb.WriteEntity(e, d) },
		list:          mongodb.ListEntities,
		query:         mongodb.ExecuteQuery,
		update:        mongodb.UpdateEntityById,
		remove:        mongodb.DeleteEntityById,
		dump:          mongodb.DumpAll,
		types:         mongodb.ListEntityTypes,
		count:         mongodb.CountAllEntities,
		read:          mongodb.ReadEntityById,
		applyIncludes: mongodb.ApplyIncludes,
		migrate:       mongodb.ExecuteMigrations,
	}

	return internal, mongo
}

func deepCopy(v any) any {
	b, _ := json.Marshal(v)
	var out any
	_ = json.Unmarshal(b, &out)

	return out
}

func copyDoc(d map[string]any) map[string]any {
	return deepCopy(d).(map[string]any)
}

func asJSON(t *testing.T, v any) string {
	t.Helper()

	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	return string(b)
}

var (
	words      = []string{"Alice", "alice", "ALICE", "Bob", "bob", "Charlie", "İstanbul", "istanbul", "\u212aate", "kate", "straße", "Go patterns", "go", "rust", "a*b", "50%", "", "x.y", "Ünïcödé", "zeta", "Zeta", "true", "false", "1", "0", "30", "2024"}
	dates      = []string{"2024-01-15", "2024-01-15T10:30:00Z", "2024-01-15T12:30:00+02:00", "2024-01-15T10:30:00.123Z", "2023-12-31T23:59:59Z", "2025-06-01", "2024-01-16T00:00:00Z", "2024-01-15T08:00:00-03:00", "1999-01-01T00:00:00.5Z"}
	numbers    = []float64{0, -0.0, 1, 2.5, 3, 10, 25, 30, 30.0, 35, -7, 1e21, 0.1}
	refAuthors = []string{"a0", "a1", "a2", "a3", "a4", "a5", "missing-author"}
)

func pick[T any](r *rand.Rand, items []T) T {
	return items[r.Intn(len(items))]
}

func randomScalar(r *rand.Rand) any {
	switch r.Intn(6) {
	case 0:
		return pick(r, words)
	case 1:
		return pick(r, dates)
	case 2:
		return pick(r, numbers)
	case 3:
		return r.Intn(2) == 0
	case 4:
		return nil
	default:
		return fmt.Sprintf("%g", pick(r, numbers))
	}
}

func randomScalarArray(r *rand.Rand) []any {
	n := r.Intn(4)
	arr := make([]any, 0, n)
	for i := 0; i < n; i++ {
		arr = append(arr, randomScalar(r))
	}

	return arr
}

func maybeSet(r *rand.Rand, doc map[string]any, key string, value func() any) {
	switch r.Intn(10) {
	case 0:
	case 1:
		doc[key] = nil
	default:
		doc[key] = value()
	}
}

func authorRef(r *rand.Rand) map[string]any {
	return map[string]any{"@entity": "author", "id": pick(r, refAuthors)}
}

func seedDataset(r *rand.Rand) [][2]any {
	writes := make([][2]any, 0)

	for i := 0; i < 4; i++ {
		company := map[string]any{"id": fmt.Sprintf("c%d", i)}
		maybeSet(r, company, "name", func() any { return pick(r, words) })
		maybeSet(r, company, "founded", func() any { return pick(r, dates) })
		writes = append(writes, [2]any{"company", company})
	}

	for i := 0; i < 70; i++ {
		post := map[string]any{"id": fmt.Sprintf("p%02d", i)}
		maybeSet(r, post, "title", func() any {
			if r.Intn(5) == 0 {
				return randomScalar(r)
			}
			return pick(r, words)
		})
		maybeSet(r, post, "score", func() any { return randomScalar(r) })
		maybeSet(r, post, "published", func() any {
			if r.Intn(4) == 0 {
				return pick(r, []any{"true", float64(1), "yes"})
			}
			return r.Intn(2) == 0
		})
		maybeSet(r, post, "created", func() any {
			if r.Intn(4) == 0 {
				return pick(r, words)
			}
			return pick(r, dates)
		})
		maybeSet(r, post, "tags", func() any {
			if r.Intn(6) == 0 {
				return pick(r, words)
			}
			return randomScalarArray(r)
		})
		maybeSet(r, post, "meta", func() any {
			if r.Intn(6) == 0 {
				return pick(r, words)
			}
			meta := map[string]any{}
			maybeSet(r, meta, "views", func() any { return randomScalar(r) })
			maybeSet(r, meta, "level", func() any { return pick(r, words) })
			return meta
		})
		maybeSet(r, post, "items", func() any {
			items := []any{}
			for j := r.Intn(3); j > 0; j-- {
				items = append(items, map[string]any{"name": pick(r, words), "qty": pick(r, numbers)})
			}
			return items
		})
		if r.Intn(4) != 0 {
			post["author"] = authorRef(r)
		}
		if r.Intn(3) == 0 {
			refs := []any{}
			for j := r.Intn(3); j > 0; j-- {
				refs = append(refs, authorRef(r))
			}
			post["authors"] = refs
		}
		if r.Intn(5) == 0 {
			post["_id"] = randomScalar(r)
		}
		maybeSet(r, post, "mixed", func() any {
			switch r.Intn(3) {
			case 0:
				return map[string]any{"k": randomScalar(r)}
			case 1:
				return []any{randomScalarArray(r), randomScalar(r)}
			}
			return randomScalar(r)
		})
		writes = append(writes, [2]any{"post", post})
	}

	for i := 0; i < 6; i++ {
		author := map[string]any{"id": fmt.Sprintf("a%d", i)}
		maybeSet(r, author, "name", func() any { return pick(r, words) })
		maybeSet(r, author, "age", func() any { return pick(r, numbers) })
		maybeSet(r, author, "tags", func() any { return randomScalarArray(r) })
		maybeSet(r, author, "company", func() any {
			return map[string]any{"@entity": "company", "id": fmt.Sprintf("c%d", r.Intn(5))}
		})
		if r.Intn(3) == 0 {
			author["friend"] = authorRef(r)
		}
		writes = append(writes, [2]any{"author", author})
	}

	return writes
}

func writeDataset(engines []engineUnderTest, writes [][2]any) {
	for _, w := range writes {
		for _, e := range engines {
			e.write(w[0].(string), copyDoc(w[1].(map[string]any)))
		}
	}
}

var filterFields = []string{"title", "score", "published", "created", "tags", "meta", "meta.views", "meta.level", "items", "items.name", "author", "author.name", "author.age", "author.tags", "author.company.name", "author.company.founded", "authors", "mixed", "id", "_id", "nope", "author.id"}

var filterOps = []string{"eq", "eq", "eq", "neq", "gt", "gte", "lt", "lte", "contains", "not_contains", "any", "all", "none", "unknown"}

func randomFilterValue(r *rand.Rand) string {
	switch r.Intn(12) {
	case 0:
		return strings.ToUpper(pick(r, words))
	case 1:
		w := pick(r, words)
		if len(w) > 1 {
			return "*" + w[1:]
		}
		return "*"
	case 2:
		return pick(r, dates)
	case 3:
		return fmt.Sprintf("%g", pick(r, numbers))
	case 4:
		return pick(r, []string{"true", "false", "1", "0", "<nil>", "map[k:x]", "[a b]", `\`, `a\*`, "NaN", "Inf", "-0", "1e21", "1000000000000000000000"})
	case 5:
		return pick(r, words) + "," + pick(r, words)
	case 6:
		return fmt.Sprintf("%g,%s", pick(r, numbers), pick(r, words))
	case 7:
		return pick(r, []string{"p0*", "a*", "*", "c1", "a3", "missing-author", "Go*"})
	default:
		return pick(r, words)
	}
}

func randomFilters(r *rand.Rand) map[string]map[string]string {
	filters := map[string]map[string]string{}
	for n := r.Intn(4); n > 0; n-- {
		field := pick(r, filterFields)
		if filters[field] == nil {
			filters[field] = map[string]string{}
		}
		filters[field][pick(r, filterOps)] = randomFilterValue(r)
	}

	return filters
}

func randomFilterNode(r *rand.Rand, depth int) query.FilterNode {
	if depth > 0 {
		switch r.Intn(4) {
		case 0:
			children := make([]query.FilterNode, 0)
			for n := r.Intn(3); n >= 0; n-- {
				children = append(children, randomFilterNode(r, depth-1))
			}
			return query.FilterNode{And: children}
		case 1:
			children := make([]query.FilterNode, 0)
			for n := r.Intn(3); n >= 0; n-- {
				children = append(children, randomFilterNode(r, depth-1))
			}
			return query.FilterNode{Or: children}
		}
	}

	return query.FilterNode{Leaf: randomFilters(r)}
}

var sortableFields = []string{"", "", "title", "score", "published", "created", "tags", "meta.views", "meta.level", "author.name", "author.age", "author.company.name", "id", "nope", "_id", "meta", "author", "authors", "mixed", "items", "items.name", "tags.0", "author.friend.name", "author.tags"}

var includeParams = []string{"", "", "", "author", "author.company", "authors", "all", "author,authors", "author.friend", "author.company,authors"}

func TestEnginesListParity(t *testing.T) {
	internal, mongo := setupEngines(t)
	r := rand.New(rand.NewSource(20261030))
	writeDataset([]engineUnderTest{internal, mongo}, seedDataset(r))

	for i := 0; i < 1000; i++ {
		limit := pick(r, []int{0, 0, 1, 3, 7, 50})
		offset := pick(r, []int{0, 0, 2, 5, 80})
		sortField := pick(r, sortableFields)
		asc := r.Intn(2) == 0
		filters := map[string]map[string]string{}
		if r.Intn(4) != 0 {
			filters = randomFilters(r)
		}
		search := ""
		if r.Intn(6) == 0 {
			search = pick(r, []string{"*li*", "go", "*a*", "2024*", "*z", "zeta", "c1", "*"})
		}
		includes := pick(r, includeParams)

		want := asJSON(t, internal.list("post", limit, offset, sortField, asc, filters, search, includes))
		got := asJSON(t, mongo.list("post", limit, offset, sortField, asc, filters, search, includes))
		if want != got {
			t.Fatalf("list mismatch #%d limit=%d offset=%d sort=%q asc=%v filters=%v search=%q includes=%q\ninternal: %s\nmongodb:  %s",
				i, limit, offset, sortField, asc, filters, search, includes, want, got)
		}
	}
}

func TestEnginesQueryParity(t *testing.T) {
	internal, mongo := setupEngines(t)
	r := rand.New(rand.NewSource(7))
	writeDataset([]engineUnderTest{internal, mongo}, seedDataset(r))

	for i := 0; i < 1000; i++ {
		q := query.Query{
			Entity: pick(r, []string{"post", "post", "author", "company", "nothing"}),
			Offset: pick(r, []int{0, 0, 1, 4, -2}),
			Limit:  pick(r, []int{0, 0, 2, 5, -1}),
			Filter: randomFilterNode(r, 3),
			Sorts:  map[string]string{},
		}
		if field := pick(r, sortableFields); field != "" {
			q.Sorts[field] = pick(r, []string{"asc", "desc", "DESC", "other"})
		}

		wantData, wantErr := internal.query(q)
		gotData, gotErr := mongo.query(q)
		want, got := asJSON(t, wantData), asJSON(t, gotData)
		if want != got || (wantErr == nil) != (gotErr == nil) {
			t.Fatalf("query mismatch #%d %s\ninternal: %s (%v)\nmongodb:  %s (%v)", i, asJSON(t, q), want, wantErr, got, gotErr)
		}
	}
}

func TestEnginesIncludesAndFieldsParity(t *testing.T) {
	internal, mongo := setupEngines(t)
	r := rand.New(rand.NewSource(99))
	writeDataset([]engineUnderTest{internal, mongo}, seedDataset(r))

	for i := 0; i < 70; i++ {
		id := fmt.Sprintf("p%02d", i)
		for _, includes := range includeParams {
			want := asJSON(t, internal.applyIncludes([]map[string]any{internal.read("post", id)}, includes))
			got := asJSON(t, mongo.applyIncludes([]map[string]any{mongo.read("post", id)}, includes))
			if want != got {
				t.Fatalf("includes mismatch for %s includes=%q\ninternal: %s\nmongodb:  %s", id, includes, want, got)
			}
		}
	}

	for i := 0; i < 70; i++ {
		id := fmt.Sprintf("p%02d", i)
		if asJSON(t, internal.read("post", id)) != asJSON(t, mongo.read("post", id)) {
			t.Fatalf("includes changed stored data of %s:\ninternal: %s\nmongodb:  %s", id, asJSON(t, internal.read("post", id)), asJSON(t, mongo.read("post", id)))
		}
	}
}

func TestEnginesMutationParity(t *testing.T) {
	internal, mongo := setupEngines(t)
	engines := []engineUnderTest{internal, mongo}
	r := rand.New(rand.NewSource(3))
	writeDataset(engines, seedDataset(r))

	compare := func(step string) {
		t.Helper()

		if want, got := asJSON(t, dumpById(internal.dump())), asJSON(t, dumpById(mongo.dump())); want != got {
			t.Fatalf("%s: dump mismatch\ninternal: %s\nmongodb:  %s", step, want, got)
		}

		wantTypes, gotTypes := internal.types(), mongo.types()
		sort.Strings(wantTypes)
		if !slices.Equal(wantTypes, gotTypes) {
			t.Fatalf("%s: entity types mismatch: internal=%v mongodb=%v", step, wantTypes, gotTypes)
		}

		if internal.count() != mongo.count() {
			t.Fatalf("%s: count mismatch: internal=%d mongodb=%d", step, internal.count(), mongo.count())
		}
	}

	compare("seed")

	for i := 0; i < 150; i++ {
		id := fmt.Sprintf("p%02d", r.Intn(80))
		switch r.Intn(4) {
		case 0:
			patch := map[string]any{"title": pick(r, words), "score": randomScalar(r)}
			if r.Intn(2) == 0 {
				patch["author"] = map[string]any{"@entity": "author", "id": fmt.Sprintf("n%d", i), "name": pick(r, words)}
			}
			if r.Intn(3) == 0 {
				patch["authors"] = []any{map[string]any{"@entity": "author", "name": "inline", "id": fmt.Sprintf("m%d", i)}, authorRef(r)}
			}
			if r.Intn(5) == 0 {
				patch["id"] = id
			}
			want := asJSON(t, internal.update("post", id, copyDoc(patch)))
			got := asJSON(t, mongo.update("post", id, copyDoc(patch)))
			if want != got {
				t.Fatalf("update %s mismatch\ninternal: %s\nmongodb:  %s", id, want, got)
			}
		case 1:
			for _, e := range engines {
				e.remove("post", id)
			}
		case 2:
			doc := map[string]any{"id": id, "title": pick(r, words), "created": pick(r, dates), "author": authorRef(r)}
			if r.Intn(3) == 0 {
				delete(doc, "id")
				doc["id"] = fmt.Sprintf("new%d", i)
			}
			writeDataset(engines, [][2]any{{"post", doc}})
		case 3:
			queries := []api_storage.MigrationQuery{{Entity: "post", Action: "set", Properties: map[string]any{"migrated": float64(i)}}}
			for _, e := range engines {
				if err := e.migrate(queries); err != nil {
					t.Fatalf("%s migrate: %v", e.name, err)
				}
			}
		}

		compare(fmt.Sprintf("step %d", i))

		for _, includes := range []string{"", "author", "all"} {
			want := asJSON(t, internal.list("post", 0, 0, "created", true, nil, "", includes))
			got := asJSON(t, mongo.list("post", 0, 0, "created", true, nil, "", includes))
			if want != got {
				t.Fatalf("step %d sorted list mismatch includes=%q\ninternal: %s\nmongodb:  %s", i, includes, want, got)
			}
		}
	}
}

func dumpById(dump map[string]any) map[string]any {
	out := make(map[string]any, len(dump))
	for entity, list := range dump {
		items := slices.Clone(list.([]map[string]any))
		slices.SortFunc(items, func(a, b map[string]any) int {
			return strings.Compare(fmt.Sprint(a["id"]), fmt.Sprint(b["id"]))
		})
		out[entity] = items
	}

	return out
}

func TestPrefilterNeverDropsAMatch(t *testing.T) {
	db := mongotest.Use(t)
	globals.SetConfig(&configuration.Config{})
	r := rand.New(rand.NewSource(11))

	collection := db.Collection("doc")
	docs := make([]map[string]any, 0)
	for i := 0; i < 250; i++ {
		doc := map[string]any{"id": fmt.Sprintf("d%03d", i)}
		for _, field := range []string{"title", "score", "published", "created", "tags", "meta", "items", "author", "mixed"} {
			maybeSet(r, doc, field, func() any {
				switch r.Intn(7) {
				case 0:
					return randomScalarArray(r)
				case 1:
					return map[string]any{"views": randomScalar(r), "level": randomScalar(r), "name": randomScalar(r)}
				case 2:
					return []any{map[string]any{"name": randomScalar(r)}, randomScalar(r)}
				case 3:
					return map[string]any{"@entity": "author", "id": pick(r, refAuthors)}
				}
				return randomScalar(r)
			})
		}

		raw := mongodb.ToMongoDocument(copyDoc(doc))
		if r.Intn(8) == 0 {
			raw["created"] = bson.NewDateTimeFromTime(mustParseTime(pick(r, dates)))
		}

		if _, err := collection.InsertOne(context.Background(), raw); err != nil {
			t.Fatalf("insert: %v", err)
		}

		stored := map[string]any{}
		if err := collection.FindOne(context.Background(), bson.M{"_id": doc["id"]}).Decode(&stored); err != nil {
			t.Fatalf("read back: %v", err)
		}
		docs = append(docs, mongodb.NormalizeMongoDocument(stored))
	}

	fields := append(slices.Clone(filterFields), "meta.name", "items.name", "tags.x", "author.@entity")

	candidates := func(filter bson.M) map[string]bool {
		t.Helper()

		cur, err := collection.Find(context.Background(), filter)
		if err != nil {
			t.Fatalf("find with %v: %v", filter, err)
		}
		defer cur.Close(context.Background())

		out := map[string]bool{}
		for cur.Next(context.Background()) {
			var raw map[string]any
			if err := cur.Decode(&raw); err != nil {
				t.Fatalf("decode: %v", err)
			}
			out[raw["_id"].(string)] = true
		}

		return out
	}

	checked, narrowed := 0, 0
	for i := 0; i < 1000; i++ {
		filters := map[string]map[string]string{}
		for n := r.Intn(3) + 1; n > 0; n-- {
			field := pick(r, fields)
			if filters[field] == nil {
				filters[field] = map[string]string{}
			}
			filters[field][pick(r, filterOps)] = randomFilterValue(r)
		}

		for _, narrowArrays := range []bool{true, false} {
			got := candidates(mongodb.BuildListPrefilter(filters, narrowArrays))
			for _, doc := range docs {
				if api_storage.FiltersMatchEntity(doc, filters) && !got[doc["id"].(string)] {
					t.Fatalf("prefilter dropped %s for %v (narrowArrays=%v): %s", doc["id"], filters, narrowArrays, asJSON(t, doc))
				}
			}
			checked++
			if len(got) < len(docs) {
				narrowed++
			}
		}

		node := randomFilterNode(r, 2)
		got := candidates(mongodb.BuildQueryPrefilter(node))
		for _, doc := range api_storage.ApplyQueryFilter(docs, node) {
			if !got[doc["id"].(string)] {
				t.Fatalf("query prefilter dropped %s for %s: %s", doc["id"], asJSON(t, node), asJSON(t, doc))
			}
		}
	}

	t.Logf("%d prefilters checked, %d narrowed the candidate set", checked, narrowed)
}

func mustParseTime(s string) time.Time {
	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t
	}

	t, err := time.Parse("2006-01-02", s)
	if err != nil {
		panic(err)
	}

	return t
}
