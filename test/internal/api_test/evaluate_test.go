package api_test

import (
	"fmt"
	"math/rand"
	"reflect"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/query"
)

var (
	evalDates = []string{
		"2015-01-01", "2014-12-31", "2015-01-02", "2022-01-01", "2021-12-31",
		"2015-01-01T00:30:00+02:00", "2014-12-31T23:59:59Z", "2015-01-01T00:00:00Z",
		"2021-12-31T22:00:00-03:00", "2022-01-01T00:00:00.5Z", "2015-01-01T23:59:59.999999999Z",
		"0000-01-01", "9999-12-31",
	}
	evalStrings = []string{"alpha", "2015", "2015-01-0", "zzz", "", "a\x00b", "Été", "2015-01-01x", "2015-13-01"}
	evalCmps    = []string{"2015-01-01", "2022-01-01", "2014-12-31", "2015-01-01T12:00:00Z", "alpha", "2015", "42", "true", "zzz", "a*", "*0*", "0000-01-01"}
	evalOps     = []string{"gte", "gt", "lt", "lte", "eq", "neq", "contains", "not_contains", "any", "all", "none"}
)

func evalValue(r *rand.Rand, datesOnly bool) any {
	if datesOnly {
		return evalDates[r.Intn(len(evalDates)-2)]
	}

	switch r.Intn(8) {
	case 7:
		return []any{}
	case 0, 1, 2:
		return evalDates[r.Intn(len(evalDates))]
	case 3:
		return evalStrings[r.Intn(len(evalStrings))]
	case 4:
		return float64(r.Intn(60)) - 10
	case 5:
		return r.Intn(2) == 0
	default:
		return []any{evalDates[r.Intn(len(evalDates))], float64(r.Intn(3)), evalStrings[r.Intn(len(evalStrings))]}
	}
}

func evalFilter(r *rand.Rand) map[string]map[string]string {
	field := []string{"day", "mixed", "rows.at"}[r.Intn(3)]
	ops := map[string]string{}
	for n := 1 + r.Intn(2); n > 0; n-- {
		ops[evalOps[r.Intn(len(evalOps))]] = evalCmps[r.Intn(len(evalCmps))]
	}
	if r.Intn(2) == 0 {
		ops = map[string]string{[]string{"gte", "gt", "lt", "lte"}[r.Intn(4)]: evalCmps[r.Intn(3)]}
		if r.Intn(2) == 0 {
			ops[[]string{"gte", "gt", "lt", "lte"}[r.Intn(4)]] = evalCmps[r.Intn(3)]
		}
	}

	return map[string]map[string]string{field: ops}
}

func waitForIndexes(t *testing.T) {
	t.Helper()

	deadline := time.Now().Add(20 * time.Second)
	for api_storage.IndexBacklog() > 0 {
		if time.Now().After(deadline) {
			t.Fatalf("the index backlog never drained")
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func TestFiltersEvaluatedOnTheIndexMatchTheScan(t *testing.T) {
	initIdxTestStore(t)

	r := rand.New(rand.NewSource(7))
	all := []map[string]any{}
	for i := 0; i < 3000; i++ {
		doc := map[string]any{"id": fmt.Sprintf("e%04d", i)}
		if r.Intn(10) > 0 {
			doc["day"] = evalValue(r, true)
		}
		if r.Intn(10) > 0 {
			doc["mixed"] = evalValue(r, false)
		}
		if r.Intn(3) > 0 {
			doc["rows"] = []any{map[string]any{"at": evalValue(r, r.Intn(2) == 0)}, map[string]any{"at": evalValue(r, false)}}
		}
		all = append(all, doc)
	}
	api_storage.WriteListOfEntities("evaluated", all)
	reference := api_storage.ListEntities("evaluated", 0, 0, "", true, nil, "", "")

	for _, field := range []string{"day", "mixed", "rows.at"} {
		api_storage.Plan(api_storage.ListQuery{Entity: "evaluated", Filters: map[string]map[string]string{field: {"eq": "x"}}})
	}
	waitForIndexes(t)

	single := []map[string]map[string]string{}
	for _, field := range []string{"day", "mixed", "rows.at"} {
		for _, op := range evalOps {
			for _, cmp := range evalCmps {
				single = append(single, map[string]map[string]string{field: {op: cmp}})
			}
		}
	}

	for i := 0; i < len(single)+1500; i++ {
		var filters map[string]map[string]string
		if i < len(single) {
			filters = single[i]
		} else {
			filters = evalFilter(r)
		}
		want := []string{}
		for _, doc := range reference {
			if api_storage.FiltersMatchEntity(doc, filters) {
				want = append(want, doc["id"].(string))
			}
		}

		got := ids(api_storage.ListEntities("evaluated", 0, 0, "", true, filters, "", ""))
		if !reflect.DeepEqual(got, want) && !(len(got) == 0 && len(want) == 0) {
			t.Fatalf("filters %q: planner %d documents, scan %d", filters, len(got), len(want))
		}

		if res := api_storage.Plan(api_storage.ListQuery{Entity: "evaluated", Filters: filters, CountOnly: true}); res.Total != len(want) {
			t.Fatalf("filters %q: count %d, scan %d", filters, res.Total, len(want))
		}

		node := query.FilterNode{Or: []query.FilterNode{{Leaf: filters}, {Leaf: evalFilter(r)}}}
		wantNode := 0
		for _, doc := range reference {
			if api_storage.QueryFilterMatchesEntity(node, doc) {
				wantNode++
			}
		}
		if res := api_storage.PlanQuery(query.Query{Entity: "evaluated", Filter: node}, true); res.Total != wantNode {
			t.Fatalf("node %+v: count %d, scan %d", node, res.Total, wantNode)
		}
	}
}

func TestDateRangeCountIsServedByTheIndex(t *testing.T) {
	initIdxTestStore(t)

	start := time.Date(2010, 1, 1, 0, 0, 0, 0, time.UTC)
	all := make([]map[string]any, 0, 20000)
	for i := 0; i < 20000; i++ {
		day := start.Add(time.Duration(i) * 6 * time.Hour)
		all = append(all, map[string]any{"id": fmt.Sprintf("v%05d", i), "date_start": day.Format("2006-01-02"), "note": "viewing"})
	}
	api_storage.WriteListOfEntities("viewing", all)

	filters := map[string]map[string]string{"date_start": {"gte": "2015-01-01"}}
	api_storage.Plan(api_storage.ListQuery{Entity: "viewing", Filters: filters, CountOnly: true})
	waitForIndexes(t)

	want := 0
	for _, doc := range all {
		if doc["date_start"].(string) >= "2015-01-01" {
			want++
		}
	}

	var decoded int64
	guard := &api_storage.Guard{Abandoned: func() bool { decoded++; return false }}
	res := api_storage.Plan(api_storage.ListQuery{Entity: "viewing", Filters: filters, CountOnly: true, Guard: guard})
	if res.Total != want {
		t.Fatalf("count %d, want %d", res.Total, want)
	}
	if decoded != 0 {
		t.Fatalf("a day range on a date field is counted on the index, %d guard checks while verifying documents", decoded)
	}
}
