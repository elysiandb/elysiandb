package api_test

import (
	"fmt"
	"math/rand"
	"reflect"
	"strings"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/storage"
)

func indexKinds(entity string) map[string]bool {
	out := map[string]bool{}
	for _, name := range storage.Engine().IndexNames(entity) {
		out[name] = true
	}
	return out
}

func TestTheAdvisorTurnsLowCardinalityFiltersIntoBitmaps(t *testing.T) {
	initIdxTestStore(t)

	docs := make([]map[string]any, 0, 6000)
	for i := 0; i < 6000; i++ {
		docs = append(docs, map[string]any{"id": fmt.Sprintf("a%05d", i), "status": []string{"open", "closed", "Draft"}[i%3], "code": fmt.Sprintf("c%05d", i)})
	}
	api_storage.WriteListOfEntities("adv", docs)

	filters := map[string]map[string]string{"status": {"eq": "draft"}}
	before := ids(api_storage.ListEntities("adv", 0, 0, "", true, filters, "", ""))
	api_storage.ListEntities("adv", 0, 0, "", true, map[string]map[string]string{"code": {"eq": "c00042"}}, "", "")

	e := storage.Engine()
	e.Advise()
	e.Advise()

	kinds := indexKinds("adv")
	if !kinds["b:status"] || kinds["f:status"] {
		t.Fatalf("a field with three values should end up as a bitmap only, got %v", kinds)
	}
	if !kinds["f:code"] || kinds["b:code"] {
		t.Fatalf("a field with distinct values stays ordered, got %v", kinds)
	}

	after := ids(api_storage.ListEntities("adv", 0, 0, "", true, filters, "", ""))
	if !reflect.DeepEqual(before, after) || len(after) != 2000 {
		t.Fatalf("the bitmap answers like the ordered index: %d before, %d after", len(before), len(after))
	}

	for i := 0; i < 6000; i++ {
		docs[i]["status"] = fmt.Sprintf("s%05d", i)
	}
	api_storage.WriteListOfEntities("adv", docs)
	e.Advise()
	e.Advise()

	kinds = indexKinds("adv")
	if !kinds["f:status"] || kinds["b:status"] {
		t.Fatalf("a field whose values became distinct goes back to an ordered index, got %v", kinds)
	}

	got := ids(api_storage.ListEntities("adv", 0, 0, "", true, map[string]map[string]string{"status": {"eq": "s00007"}}, "", ""))
	if len(got) != 1 || got[0] != "a00007" {
		t.Fatalf("filter after the switch back: %v", got)
	}
}

func TestCompositeIndexesPageLikeTheSortIndex(t *testing.T) {
	initIdxTestStore(t)

	r := rand.New(rand.NewSource(3))
	kinds := []string{"film", "Serie", "doc", "Film"}
	makeDoc := func(i int) map[string]any {
		doc := map[string]any{"id": fmt.Sprintf("w%05d", i), "kind": kinds[r.Intn(len(kinds))]}
		switch r.Intn(5) {
		case 0:
		case 1:
			doc["rank"] = float64(r.Intn(50))
		case 2:
			doc["rank"] = fmt.Sprintf("t%02d", r.Intn(30))
		default:
			doc["rank"] = float64(r.Intn(20))
		}
		return doc
	}

	docs := make([]map[string]any, 0, 5000)
	for i := 0; i < 5000; i++ {
		docs = append(docs, makeDoc(i))
	}
	api_storage.WriteListOfEntities("cmp", docs)

	check := func(label string) {
		t.Helper()
		all := api_storage.ListEntities("cmp", 0, 0, "", true, nil, "", "")
		for _, kind := range []string{"film", "FILM", "serie", "doc", "none"} {
			for _, asc := range []bool{true, false} {
				filters := map[string]map[string]string{"kind": {"eq": kind}}
				want := []map[string]any{}
				for _, doc := range all {
					if api_storage.FiltersMatchEntity(doc, filters) {
						want = append(want, doc)
					}
				}
				api_storage.SortEntities(want, []query.Sort{{Field: "rank", Ascending: asc}})
				wantIDs := ids(want)

				for _, page := range [][2]int{{0, 0}, {0, 20}, {37, 25}, {len(wantIDs) - 3, 10}, {len(wantIDs) + 5, 10}} {
					res := api_storage.Plan(api_storage.ListQuery{Entity: "cmp", Filters: filters, SortField: "rank", SortAsc: asc, Offset: page[0], Limit: page[1]})
					start := min(max(page[0], 0), len(wantIDs))
					end := len(wantIDs)
					if page[1] > 0 {
						end = min(start+page[1], len(wantIDs))
					}
					if got := res.IDs(); !reflect.DeepEqual(got, wantIDs[start:end]) && !(len(got) == 0 && start == end) {
						t.Fatalf("%s: kind %s asc %v page %v: got %v, want %v", label, kind, asc, page, got, wantIDs[start:end])
					}
				}

				count := api_storage.Plan(api_storage.ListQuery{Entity: "cmp", Filters: filters, SortField: "rank", SortAsc: asc, CountOnly: true})
				if count.Total != len(wantIDs) {
					t.Fatalf("%s: count for %s = %d, want %d", label, kind, count.Total, len(wantIDs))
				}

				node := query.FilterNode{Leaf: filters}
				qres := api_storage.PlanQuery(query.Query{Entity: "cmp", Filter: node, Sorts: map[string]string{"rank": map[bool]string{true: "asc", false: "desc"}[asc]}, Offset: 11, Limit: 7}, false)
				end := min(18, len(wantIDs))
				start := min(11, end)
				if got := qres.IDs(); !reflect.DeepEqual(got, wantIDs[start:end]) && !(len(got) == 0 && start == end) {
					t.Fatalf("%s: query API kind %s asc %v: got %v, want %v", label, kind, asc, got, wantIDs[start:end])
				}
			}
		}
	}

	check("without composite")

	e := storage.Engine()
	if !e.EnsureIndex("cmp", datastore.CompositeSpec("kind", "rank")) {
		t.Fatalf("composite not built")
	}
	check("with composite")

	uses := int64(0)
	for _, st := range e.IndexStates() {
		if st.Entity == "cmp" && st.Index == "c:kind,rank" {
			uses = st.Uses
		}
	}
	if uses < 40 {
		t.Fatalf("the pages should be read from the composite index, used %d times", uses)
	}

	for i := 0; i < 2000; i++ {
		n := r.Intn(5000)
		if r.Intn(4) == 0 {
			api_storage.DeleteEntityById("cmp", fmt.Sprintf("w%05d", n))
			continue
		}
		api_storage.WriteEntity("cmp", makeDoc(n))
	}
	check("after writes")

	api_storage.WriteEntity("cmp", map[string]any{"id": "odd", "kind": true, "rank": 1.0})
	check("with an irregular document")
	api_storage.DeleteEntityById("cmp", "odd")
	check("after the irregular document left")
}

func TestQueryShapesCreateCompositeIndexes(t *testing.T) {
	initIdxTestStore(t)

	docs := make([]map[string]any, 0, 5000)
	for i := 0; i < 5000; i++ {
		docs = append(docs, map[string]any{"id": fmt.Sprintf("v%05d", i), "lang": []string{"vo", "vf"}[i%2], "day": fmt.Sprintf("d%05d", 5000-i)})
	}
	api_storage.WriteListOfEntities("shape", docs)

	for i := 0; i < 10; i++ {
		api_storage.ListEntities("shape", 10, i*10, "day", true, map[string]map[string]string{"lang": {"eq": "vo"}}, "", "")
	}

	if !indexKinds("shape")["c:lang,day"] {
		t.Fatalf("a repeated filter and sort shape should get a composite index, got %v", indexKinds("shape"))
	}

	got := ids(api_storage.ListEntities("shape", 3, 2, "day", true, map[string]map[string]string{"lang": {"eq": "VO"}}, "", ""))
	want := []string{"v04994", "v04992", "v04990"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("composite page %v, want %v", got, want)
	}

}
