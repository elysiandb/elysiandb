package api_test

import (
	"bytes"
	"fmt"
	"math/rand"
	"reflect"
	"sort"
	"strings"
	"sync"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/storage"
)

func initIdxTestStore(t *testing.T) {
	t.Helper()
	tmp := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder: tmp,
			Shards: 8,
		},
		Stats: configuration.StatsConfig{
			Enabled: false,
		},
	})
	storage.LoadDB()
	storage.LoadJsonDB()
	t.Cleanup(func() { _ = storage.Close() })
}

func decodeIDs(b []byte) []string {
	if len(b) == 0 {
		return nil
	}
	parts := bytes.Split(b, []byte{'\n'})
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if len(p) > 0 {
			out = append(out, string(p))
		}
	}
	return out
}

func ids(list []map[string]any) []string {
	out := make([]string, 0, len(list))
	for _, item := range list {
		out = append(out, item["id"].(string))
	}

	return out
}

func TestConcurrentWritesToOneEntityLoseNoID(t *testing.T) {
	initIdxTestStore(t)

	api_storage.WriteEntity("items", map[string]any{"id": "seed", "n": -1.0})
	api_storage.EnsureSortIndex("items", "n")

	var wg sync.WaitGroup
	for g := 0; g < 16; g++ {
		wg.Add(1)
		go func(g int) {
			defer wg.Done()
			for i := 0; i < 200; i++ {
				api_storage.WriteEntity("items", map[string]any{"id": fmt.Sprintf("g%02d-%03d", g, i), "n": float64(g*1000 + i)})
			}
		}(g)
	}
	wg.Wait()

	want := 3201
	if got := api_storage.CountEntities("items"); got != want {
		t.Fatalf("count %d, want %d", got, want)
	}

	if got := len(api_storage.ListEntities("items", 0, 0, "", true, nil, "", "")); got != want {
		t.Fatalf("list %d, want %d", got, want)
	}

	if got := len(api_storage.ListEntities("items", 0, 0, "n", true, nil, "", "")); got != want {
		t.Fatalf("sorted list %d, want %d", got, want)
	}

	raw, err := api_storage.GetListOfIds("items", "", true)
	if err != nil || len(decodeIDs(raw)) != want {
		t.Fatalf("id index %d, want %d (%v)", len(decodeIDs(raw)), want, err)
	}
}

func TestWriteDuringAnIndexBuildIsInTheIndex(t *testing.T) {
	initIdxTestStore(t)

	docs := make([]map[string]any, 0, 20000)
	for i := 0; i < 20000; i++ {
		docs = append(docs, map[string]any{"id": fmt.Sprintf("d%05d", i), "rank": float64(i)})
	}
	api_storage.WriteListOfEntities("big", docs)

	done := make(chan struct{})
	go func() {
		defer close(done)
		api_storage.WriteEntity("big", map[string]any{"id": "late", "rank": -1.0})
	}()

	sorted := api_storage.ListEntities("big", 0, 0, "rank", true, nil, "", "")
	<-done

	sorted = api_storage.ListEntities("big", 0, 0, "rank", true, nil, "", "")
	if len(sorted) != 20001 || sorted[0]["id"] != "late" {
		t.Fatalf("expected 20001 sorted documents starting with the late write, got %d, first %v", len(sorted), sorted[0]["id"])
	}

	if got := api_storage.CountEntities("big"); got != 20001 {
		t.Fatalf("count %d", got)
	}
}

func TestSortTiesComeInInsertionOrderAndMissingValuesLast(t *testing.T) {
	initIdxTestStore(t)

	for _, d := range []map[string]any{
		{"id": "s1", "v": "b"},
		{"id": "n2", "v": 2.0},
		{"id": "m1"},
		{"id": "s2", "v": "a"},
		{"id": "n1", "v": 1.0},
		{"id": "d1", "v": "2024-01-02"},
		{"id": "b1", "v": true},
		{"id": "s3", "v": "a"},
		{"id": "m2", "v": nil},
		{"id": "o1", "v": map[string]any{"x": 1.0}},
		{"id": "d2", "v": "2023-05-01T10:00:00+02:00"},
		{"id": "b2", "v": false},
		{"id": "n3", "v": 1.0},
	} {
		api_storage.WriteEntity("mixed", d)
	}

	asc := ids(api_storage.ListEntities("mixed", 0, 0, "v", true, nil, "", ""))
	wantAsc := []string{"n1", "n3", "n2", "d2", "d1", "s2", "s3", "s1", "b2", "b1", "o1", "m1", "m2"}
	if !reflect.DeepEqual(asc, wantAsc) {
		t.Fatalf("asc %v, want %v", asc, wantAsc)
	}

	desc := ids(api_storage.ListEntities("mixed", 0, 0, "v", false, nil, "", ""))
	wantDesc := []string{"o1", "b1", "b2", "s1", "s2", "s3", "d1", "d2", "n2", "n1", "n3", "m1", "m2"}
	if !reflect.DeepEqual(desc, wantDesc) {
		t.Fatalf("desc %v, want %v", desc, wantDesc)
	}

	for offset := 0; offset < len(wantDesc); offset++ {
		page := ids(api_storage.ListEntities("mixed", 3, offset, "v", false, nil, "", ""))
		end := min(offset+3, len(wantDesc))
		if !reflect.DeepEqual(page, wantDesc[offset:end]) {
			t.Fatalf("desc page at %d: %v, want %v", offset, page, wantDesc[offset:end])
		}
	}

	data := api_storage.ListEntities("mixed", 0, 0, "", true, nil, "", "")
	api_storage.SortEntities(data, []query.Sort{{Field: "v", Ascending: false}})
	if got := ids(data); !reflect.DeepEqual(got, wantDesc) {
		t.Fatalf("the Query API comparator gives %v, the index %v", got, wantDesc)
	}
}

func TestIndexesFollowUpdatesDeletesAndResets(t *testing.T) {
	initIdxTestStore(t)

	for i := 0; i < 50; i++ {
		api_storage.WriteEntity("users", map[string]any{"id": fmt.Sprintf("u%02d", i), "age": float64(50 - i), "team": fmt.Sprintf("t%d", i%5)})
	}

	filters := map[string]map[string]string{"team": {"eq": "t1"}}
	if got := len(api_storage.ListEntities("users", 0, 0, "age", true, filters, "", "")); got != 10 {
		t.Fatalf("expected 10 members of t1, got %d", got)
	}

	api_storage.UpdateEntityById("users", "u01", map[string]any{"team": "t2", "age": 1000.0})
	api_storage.DeleteEntityById("users", "u06")

	got := ids(api_storage.ListEntities("users", 0, 0, "age", true, filters, "", ""))
	if len(got) != 8 || got[0] != "u46" {
		t.Fatalf("unexpected t1 members after an update and a delete: %v", got)
	}

	sorted := ids(api_storage.ListEntities("users", 1, 0, "age", false, nil, "", ""))
	if !reflect.DeepEqual(sorted, []string{"u01"}) {
		t.Fatalf("the updated age leads the descending sort, got %v", sorted)
	}

	api_storage.DeleteAllEntities("users")
	if got := api_storage.ListEntities("users", 0, 0, "age", true, filters, "", ""); len(got) != 0 {
		t.Fatalf("a reset empties every index, got %v", got)
	}

	api_storage.WriteEntity("users", map[string]any{"id": "new", "age": 3.0, "team": "t1"})
	if got := ids(api_storage.ListEntities("users", 0, 0, "age", true, filters, "", "")); !reflect.DeepEqual(got, []string{"new"}) {
		t.Fatalf("indexes work after a reset, got %v", got)
	}
}

func TestIndexKeysForTheDocumentedEntityKeys(t *testing.T) {
	initIdxTestStore(t)

	api_storage.WriteEntity("books", map[string]any{"id": "b2", "title": "Zed"})
	api_storage.WriteEntity("books", map[string]any{"id": "b1", "title": "Ada"})
	api_storage.ListEntities("books", 0, 0, "title", true, nil, "", "")

	raw, err := storage.GetByKey(globals.ApiEntityIndexIdKey("books"))
	if err != nil || strings.Join(decodeIDs(raw), ",") != "b2,b1" {
		t.Fatalf("id index %q %v", raw, err)
	}

	raw, err = storage.GetByKey(globals.ApiEntityIndexFieldSortAscKey("books", "title"))
	if err != nil || strings.Join(decodeIDs(raw), ",") != "b1,b2" {
		t.Fatalf("sort index %q %v", raw, err)
	}

	raw, err = storage.GetByKey(globals.ApiAllEntityTypesListKey())
	if err != nil || string(raw) != "books" {
		t.Fatalf("types list %q %v", raw, err)
	}

	if !api_storage.IndexExistsForField("books", "title") || !reflect.DeepEqual(api_storage.GetListForIndexedFields("books"), []string{"title"}) {
		t.Fatalf("the sort index of title should be listed")
	}

	if got := api_storage.GetSortedEntityIdsByField("books", "title", false); !reflect.DeepEqual(got, []string{"b2", "b1"}) {
		t.Fatalf("descending ids %v", got)
	}
}

var (
	diffStrings = []string{"alpha", "Alpha", "beta", "BETA gamma", "2024-01-15", "2024-01-15T08:30:00Z", "2024-01-15T23:30:00-05:00", "2023-12-31", "", "a*b", "x\\y", "42", "true", "Ünïcode"}
	diffOps     = []string{"eq", "neq", "lt", "lte", "gt", "gte", "contains", "not_contains", "all", "any", "none", "weird"}
	diffCmps    = []string{"alpha", "ALPHA", "al*", "*ta", "*", "b*a", "2024-01-15", "2024-01-15T10:00:00Z", "2023-06-01", "42", "41.5", "true", "1", "0", "false", "beta,alpha", "1,2", "", "a\\*b", "ü*"}
)

func randomValue(r *rand.Rand, depth int) any {
	switch r.Intn(9) {
	case 0, 1:
		return diffStrings[r.Intn(len(diffStrings))]
	case 2:
		return float64(r.Intn(100)) - 20 + []float64{0, 0.5}[r.Intn(2)]
	case 3:
		return r.Intn(2) == 0
	case 4:
		n := r.Intn(4)
		arr := make([]any, 0, n)
		for i := 0; i < n; i++ {
			if r.Intn(3) == 0 {
				arr = append(arr, float64(r.Intn(3)))
			} else {
				arr = append(arr, diffStrings[r.Intn(len(diffStrings))])
			}
		}
		return arr
	case 5:
		if depth > 1 {
			return nil
		}
		return map[string]any{"x": randomValue(r, depth+1), "y": randomValue(r, depth+1)}
	case 6:
		if depth > 1 {
			return "leaf"
		}
		return []any{map[string]any{"x": randomValue(r, depth+1)}, map[string]any{"x": randomValue(r, depth+1)}}
	case 7:
		return nil
	default:
		return float64(r.Intn(5))
	}
}

func randomLeaf(r *rand.Rand) map[string]map[string]string {
	field := []string{"a", "b", "m.x", "m.y", "list.x"}[r.Intn(5)]
	ops := map[string]string{diffOps[r.Intn(len(diffOps))]: diffCmps[r.Intn(len(diffCmps))]}
	if r.Intn(4) == 0 {
		ops[diffOps[r.Intn(len(diffOps))]] = diffCmps[r.Intn(len(diffCmps))]
	}

	filters := map[string]map[string]string{field: ops}
	if r.Intn(3) == 0 {
		other := []string{"a", "b", "m.x"}[r.Intn(3)]
		if _, exists := filters[other]; !exists {
			filters[other] = map[string]string{diffOps[r.Intn(4)]: diffCmps[r.Intn(len(diffCmps))]}
		}
	}

	return filters
}

func TestPlannerMatchesTheScanOnRandomDocumentsAndFilters(t *testing.T) {
	initIdxTestStore(t)

	r := rand.New(rand.NewSource(42))
	all := []map[string]any{}
	for i := 0; i < 600; i++ {
		doc := map[string]any{"id": fmt.Sprintf("r%03d", i)}
		for _, f := range []string{"a", "b", "m", "list"} {
			if r.Intn(6) == 0 {
				continue
			}

			switch f {
			case "m":
				doc[f] = map[string]any{"x": randomValue(r, 1), "y": randomValue(r, 1)}
			case "list":
				doc[f] = []any{map[string]any{"x": randomValue(r, 1)}, map[string]any{"x": randomValue(r, 1)}}
			default:
				doc[f] = randomValue(r, 0)
			}
		}
		all = append(all, doc)
	}

	api_storage.WriteListOfEntities("diff", all)
	reference := api_storage.ListEntities("diff", 0, 0, "", true, nil, "", "")

	scan := func(filters map[string]map[string]string) []string {
		out := []string{}
		for _, doc := range reference {
			if api_storage.FiltersMatchEntity(doc, filters) {
				out = append(out, doc["id"].(string))
			}
		}
		return out
	}

	for i := 0; i < 1500; i++ {
		filters := randomLeaf(r)
		want := scan(filters)
		got := ids(api_storage.ListEntities("diff", 0, 0, "", true, filters, "", ""))
		if !reflect.DeepEqual(got, want) && !(len(got) == 0 && len(want) == 0) {
			t.Fatalf("filters %v: planner %v, scan %v", filters, got, want)
		}

		res := api_storage.Plan(api_storage.ListQuery{Entity: "diff", Filters: filters, CountOnly: true})
		if res.Total != len(want) {
			t.Fatalf("filters %v: count %d, scan %d", filters, res.Total, len(want))
		}
	}

	for i := 0; i < 300; i++ {
		node := query.FilterNode{Or: []query.FilterNode{{Leaf: randomLeaf(r)}, {And: []query.FilterNode{{Leaf: randomLeaf(r)}, {Leaf: randomLeaf(r)}}}}}
		want := []string{}
		for _, doc := range reference {
			if api_storage.QueryFilterMatchesEntity(node, doc) {
				want = append(want, doc["id"].(string))
			}
		}

		got := ids(api_storage.PlanQuery(query.Query{Entity: "diff", Filter: node}, false).Decode())
		if !reflect.DeepEqual(got, want) && !(len(got) == 0 && len(want) == 0) {
			t.Fatalf("node %+v: planner %v, scan %v", node, got, want)
		}
	}
}

func TestSearchIndexMatchesTheScan(t *testing.T) {
	initIdxTestStore(t)

	r := rand.New(rand.NewSource(3))
	words := []string{"dragon", "Dune", "foundation", "kindred", "the", "abc", "Ünïcode", "a*b", "beginning"}
	docs := []map[string]any{}
	for i := 0; i < 300; i++ {
		doc := map[string]any{"id": fmt.Sprintf("s%03d", i), "title": words[r.Intn(len(words))] + " " + words[r.Intn(len(words))]}
		if r.Intn(2) == 0 {
			doc["tags"] = []any{words[r.Intn(len(words))]}
		}
		if r.Intn(3) == 0 {
			doc["meta"] = map[string]any{"note": words[r.Intn(len(words))], "_elysiandb_core_x": "dragon"}
		}
		docs = append(docs, doc)
	}
	api_storage.WriteListOfEntities("texts", docs)
	api_storage.ListEntities("texts", 0, 0, "", true, nil, "warm", "")
	storage.Engine().SearchCandidates("texts", "*warm*")

	reference := api_storage.ListEntities("texts", 0, 0, "", true, nil, "", "")
	for _, search := range []string{"dragon", "DUNE", "the", "ab", "*ing", "beg*ing", "zzz", "nïc", "a\\*b", "dune dragon", "on"} {
		want := []string{}
		for _, doc := range reference {
			if api_storage.SearchMatchesEntity(doc, search) {
				want = append(want, doc["id"].(string))
			}
		}

		got := ids(api_storage.ListEntities("texts", 0, 0, "", true, nil, search, ""))
		if !reflect.DeepEqual(got, want) && !(len(got) == 0 && len(want) == 0) {
			t.Fatalf("search %q: index %v, scan %v", search, got, want)
		}
	}

	api_storage.UpdateEntityById("texts", "s000", map[string]any{"title": "zebra crossing"})
	got := ids(api_storage.ListEntities("texts", 0, 0, "", true, nil, "zebra", ""))
	if !reflect.DeepEqual(got, []string{"s000"}) {
		t.Fatalf("an update is searchable at once, got %v", got)
	}
}

func TestUnfilteredPagesAndCountsUseTheLiveSet(t *testing.T) {
	initIdxTestStore(t)

	list := []map[string]any{}
	for i := 0; i < 1000; i++ {
		list = append(list, map[string]any{"id": fmt.Sprintf("p%04d", i)})
	}
	api_storage.WriteListOfEntities("pages", list)

	for i := 0; i < 1000; i += 3 {
		api_storage.DeleteEntityById("pages", fmt.Sprintf("p%04d", i))
	}

	remaining := []string{}
	for i := 0; i < 1000; i++ {
		if i%3 != 0 {
			remaining = append(remaining, fmt.Sprintf("p%04d", i))
		}
	}

	if got := api_storage.CountEntities("pages"); got != len(remaining) {
		t.Fatalf("count %d, want %d", got, len(remaining))
	}

	for _, offset := range []int{0, 1, 333, 665, 666, 700} {
		page := ids(api_storage.ListEntities("pages", 10, offset, "", true, nil, "", ""))
		end := min(offset+10, len(remaining))
		start := min(offset, len(remaining))
		if !reflect.DeepEqual(page, remaining[start:end]) && !(len(page) == 0 && start == end) {
			t.Fatalf("page at %d: %v, want %v", offset, page, remaining[start:end])
		}
	}

	snap := storage.Engine().Snapshot("pages")
	seqs := []int{}
	snap.Ascend(0, func(d datastore.Doc) bool {
		seqs = append(seqs, int(d.Seq()))
		return true
	})
	if !sort.IntsAreSorted(seqs) {
		t.Fatalf("the live set iterates in insertion order")
	}
}

func TestFiltersThroughReferencesMatchTheScan(t *testing.T) {
	initIdxTestStore(t)

	r := rand.New(rand.NewSource(5))
	names := []string{"Ada", "ada lovelace", "Bob", "Chiang", "Dune", "Émile"}
	for i := 0; i < 40; i++ {
		api_storage.WriteEntity("person", map[string]any{"id": fmt.Sprintf("p%02d", i), "name": names[r.Intn(len(names))], "age": float64(20 + r.Intn(50)), "city": map[string]any{"@entity": "city", "id": fmt.Sprintf("c%d", r.Intn(5))}})
	}
	for i := 0; i < 5; i++ {
		api_storage.WriteEntity("city", map[string]any{"id": fmt.Sprintf("c%d", i), "label": []string{"Paris", "Lyon", "paris", "Oslo", "Rome"}[i]})
	}

	docs := []map[string]any{}
	for i := 0; i < 300; i++ {
		doc := map[string]any{"id": fmt.Sprintf("b%03d", i), "title": names[r.Intn(len(names))]}
		switch r.Intn(4) {
		case 0:
			doc["author"] = map[string]any{"@entity": "person", "id": fmt.Sprintf("p%02d", r.Intn(45))}
		case 1:
			doc["author"] = map[string]any{"name": names[r.Intn(len(names))], "age": float64(r.Intn(90))}
		case 2:
			doc["authors"] = []any{map[string]any{"@entity": "person", "id": fmt.Sprintf("p%02d", r.Intn(40))}, map[string]any{"@entity": "person", "id": fmt.Sprintf("p%02d", r.Intn(40))}}
		}
		docs = append(docs, doc)
	}
	api_storage.WriteListOfEntities("bk", docs)

	filters := []map[string]map[string]string{
		{"author.name": {"eq": "ada*"}},
		{"author.name": {"contains": "chi"}},
		{"author.age": {"gt": "40"}},
		{"authors.name": {"eq": "Bob"}},
		{"authors.age": {"lte": "30"}, "title": {"eq": "dune"}},
		{"author.city.label": {"eq": "paris"}},
		{"authors.city.id": {"eq": "c2"}},
		{"author.name": {"neq": "Bob"}},
		{"author.id": {"eq": "p03"}},
	}

	for _, f := range filters {
		includes := api_storage.ExtractAutoIncludes(f)
		all := api_storage.ListEntities("bk", 0, 0, "", true, nil, "", includes)
		want := []string{}
		for _, doc := range all {
			if api_storage.FiltersMatchEntity(doc, f) {
				want = append(want, doc["id"].(string))
			}
		}

		got := ids(api_storage.ListEntities("bk", 0, 0, "", true, f, "", ""))
		if !reflect.DeepEqual(got, want) && !(len(got) == 0 && len(want) == 0) {
			t.Fatalf("filters %v: planner %v, scan %v", f, got, want)
		}

		if res := api_storage.Plan(api_storage.ListQuery{Entity: "bk", Filters: f, Includes: includes, CountOnly: true}); res.Total != len(want) {
			t.Fatalf("filters %v: count %d, scan %d", f, res.Total, len(want))
		}
	}

	api_storage.UpdateEntityById("person", "p03", map[string]any{"name": "Zed"})
	f := map[string]map[string]string{"author.name": {"eq": "zed"}}
	got := api_storage.ListEntities("bk", 0, 0, "", true, f, "", "")
	for _, doc := range got {
		if doc["author"].(map[string]any)["id"] != "p03" {
			t.Fatalf("an update of the target is seen through the reference, got %v", doc)
		}
	}
}
