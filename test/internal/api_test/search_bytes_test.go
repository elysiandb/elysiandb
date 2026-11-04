package api_test

import (
	"encoding/json"
	"fmt"
	"math/rand"
	"reflect"
	"strings"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
)

var searchFragments = []string{"alpha", "ÉTÉ", "été", "<b>", "a&b", "quo\"te", "back\\slash", "tab\there", "line sep", "😀 smile", "x\x01y", "\xff\xfe", "nnn", "Dune", "dune", "*", "", "42"}

func randomSearchValue(r *rand.Rand, depth int) any {
	switch r.Intn(7) {
	case 0, 1, 2:
		return searchFragments[r.Intn(len(searchFragments))] + searchFragments[r.Intn(len(searchFragments))]
	case 3:
		return float64(r.Intn(100))
	case 4:
		if depth > 2 {
			return true
		}
		arr := []any{}
		for i := r.Intn(3); i > 0; i-- {
			arr = append(arr, randomSearchValue(r, depth+1))
		}
		return arr
	case 5:
		if depth > 2 {
			return nil
		}
		return randomSearchDoc(r, depth+1)
	default:
		return nil
	}
}

func randomSearchDoc(r *rand.Rand, depth int) map[string]any {
	doc := map[string]any{}
	keys := []string{"id", "@entity", "_elysiandb_core_owner", "title", "notes", "tags", "Dune", "k\"ey"}
	for i := 1 + r.Intn(4); i > 0; i-- {
		doc[keys[r.Intn(len(keys))]] = randomSearchValue(r, depth)
	}
	return doc
}

func randomSearchPattern(r *rand.Rand) string {
	frag := searchFragments[r.Intn(len(searchFragments))]
	if len(frag) > 2 && r.Intn(2) == 0 {
		frag = frag[1 : len(frag)-1]
	}
	switch r.Intn(5) {
	case 0:
		return "*" + frag + "*"
	case 1:
		return frag + "*"
	case 2:
		return strings.ToUpper(frag)
	case 3:
		return "\\" + frag
	default:
		return frag
	}
}

func TestSearchOnStoredBytesMatchesTheDecodedSearch(t *testing.T) {
	r := rand.New(rand.NewSource(11))
	for i := 0; i < 20000; i++ {
		raw, err := json.Marshal(randomSearchDoc(r, 0))
		if err != nil {
			t.Fatal(err)
		}
		var doc map[string]any
		if err := json.Unmarshal(raw, &doc); err != nil {
			t.Fatal(err)
		}

		pattern := randomSearchPattern(r)
		got, ok := api_storage.SearchMatchesBytes(raw, pattern)
		if want := api_storage.SearchMatchesEntity(doc, pattern); !ok || got != want {
			t.Fatalf("search %q on %s: bytes %v (ok %v), decoded %v", pattern, raw, got, ok, want)
		}
	}
}

func TestSearchOnHandWrittenJSON(t *testing.T) {
	for _, c := range []struct{ raw, search string }{
		{`{"a":"\/path\/to"}`, "/path/to"},
		{`{"a":"café"}`, "café"},
		{`{"a":"CAFÉ"}`, "café"},
		{`{"a":"😀 smile"}`, "😀"},
		{`{"a":"lone \ud800 surrogate"}`, "lone �"},
		{`{"a":"lone \ud800A"}`, "�A"},
		{` { "a" : [ 1 , "x\tz" , { "b" : "deep" } ] , "id" : "deep" } `, "deep"},
		{`{"id":"only id"}`, "only"},
		{`{"a":{"@entity":"hidden","n":null}}`, "hidden"},
		{`{"a":"x\u0000y"}`, "x\u0000y"},
	} {
		var doc map[string]any
		if err := json.Unmarshal([]byte(c.raw), &doc); err != nil {
			t.Fatalf("%s: %v", c.raw, err)
		}
		got, ok := api_storage.SearchMatchesBytes([]byte(c.raw), c.search)
		if want := api_storage.SearchMatchesEntity(doc, c.search); !ok || got != want {
			t.Fatalf("search %q on %s: bytes %v (ok %v), decoded %v", c.search, c.raw, got, ok, want)
		}
	}

	for _, broken := range []string{`{"a":"x`, `{"a":"\x"}`, `{"a" "x"}`, `["a"]`, `{"a":"\u12"}`, `{"a":"it\'s"}`} {
		if _, ok := api_storage.SearchMatchesBytes([]byte(broken), "a"); ok {
			t.Fatalf("invalid JSON %s falls back to decoding", broken)
		}
	}
}

func TestSearchesAndSearchCountsMatchTheScan(t *testing.T) {
	initIdxTestStore(t)

	r := rand.New(rand.NewSource(5))
	all := []map[string]any{}
	for i := 0; i < 4000; i++ {
		doc := randomSearchDoc(r, 1)
		doc["id"] = fmt.Sprintf("s%04d", i)
		doc["kind"] = []string{"film", "serie"}[i%2]
		all = append(all, doc)
	}
	api_storage.WriteListOfEntities("searched", all)
	reference := api_storage.ListEntities("searched", 0, 0, "", true, nil, "", "")
	api_storage.ListEntities("searched", 1, 0, "", true, nil, "alpha", "")
	api_storage.ListEntities("searched", 1, 0, "", true, map[string]map[string]string{"kind": {"eq": "film"}}, "", "")
	waitForIndexes(t)

	for _, search := range []string{"alpha", "été", "ÉTÉ", "<b>", "a&b", "smile", "dune", "nnn42", "*", "zzz", `back\\slash`} {
		for _, filters := range []map[string]map[string]string{nil, {"kind": {"eq": "film"}}} {
			want := []string{}
			for _, doc := range reference {
				if api_storage.SearchMatchesEntity(doc, search) && (filters == nil || api_storage.FiltersMatchEntity(doc, filters)) {
					want = append(want, doc["id"].(string))
				}
			}

			got := ids(api_storage.ListEntities("searched", 0, 0, "", true, filters, search, ""))
			if !reflect.DeepEqual(got, want) && !(len(got) == 0 && len(want) == 0) {
				t.Fatalf("search %q filters %v: planner %d documents, scan %d", search, filters, len(got), len(want))
			}
			if res := api_storage.Plan(api_storage.ListQuery{Entity: "searched", Search: search, Filters: filters, CountOnly: true}); res.Total != len(want) {
				t.Fatalf("search %q filters %v: count %d, scan %d", search, filters, res.Total, len(want))
			}
		}
	}
}
