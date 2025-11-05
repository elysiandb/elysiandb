package e2e

import (
	"encoding/json"
	"net"
	"regexp"
	"testing"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
	"github.com/valyala/fasthttp/fasthttputil"
)

var uuidRe = regexp.MustCompile(`^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$`)

func startTestServer(t *testing.T) (*fasthttp.Client, func()) {
	t.Helper()

	tmp := t.TempDir()
	cfg := &configuration.Config{
		Store: configuration.StoreConfig{
			Folder: tmp,
			Shards: 8,
		},
		Stats: configuration.StatsConfig{
			Enabled: true,
		},
	}
	globals.SetConfig(cfg)
	storage.LoadDB()

	r := router.New()
	routing.RegisterRoutes(r)
	srv := &fasthttp.Server{Handler: r.Handler}

	ln := fasthttputil.NewInmemoryListener()
	go func() { _ = srv.Serve(ln) }()

	client := &fasthttp.Client{
		Dial: func(addr string) (net.Conn, error) { return ln.Dial() },
	}

	teardown := func() {
		_ = ln.Close()
		_ = srv.Shutdown()
	}
	return client, teardown
}

func mustPOSTJSON(t *testing.T, c *fasthttp.Client, url string, payload any) *fasthttp.Response {
	t.Helper()
	b, _ := json.Marshal(payload)
	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	req.Header.SetMethod(fasthttp.MethodPost)
	req.SetRequestURI(url)
	req.SetBody(b)
	if err := c.Do(req, resp); err != nil {
		t.Fatalf("POST %s failed: %v", url, err)
	}
	fasthttp.ReleaseRequest(req)
	return resp
}

func mustPUTJSON(t *testing.T, c *fasthttp.Client, url string, payload any) *fasthttp.Response {
	t.Helper()
	b, _ := json.Marshal(payload)
	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	req.Header.SetMethod(fasthttp.MethodPut)
	req.SetRequestURI(url)
	req.SetBody(b)
	if err := c.Do(req, resp); err != nil {
		t.Fatalf("PUT %s failed: %v", url, err)
	}
	fasthttp.ReleaseRequest(req)
	return resp
}

func mustGET(t *testing.T, c *fasthttp.Client, url string) *fasthttp.Response {
	t.Helper()
	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI(url)
	if err := c.Do(req, resp); err != nil {
		t.Fatalf("GET %s failed: %v", url, err)
	}
	fasthttp.ReleaseRequest(req)
	return resp
}

func mustDELETE(t *testing.T, c *fasthttp.Client, url string) *fasthttp.Response {
	t.Helper()
	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	req.Header.SetMethod(fasthttp.MethodDelete)
	req.SetRequestURI(url)
	if err := c.Do(req, resp); err != nil {
		t.Fatalf("DELETE %s failed: %v", url, err)
	}
	fasthttp.ReleaseRequest(req)
	return resp
}

func TestAutoREST_Create_GetAll_GetByID_Update_Delete_Destroy_All(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cr := mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "Hello",
		"tags":  []string{"a", "b"},
	})
	if sc := cr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("POST /api/articles expected 200, got %d", sc)
	}
	var created map[string]any
	if err := json.Unmarshal(cr.Body(), &created); err != nil {
		t.Fatalf("invalid JSON create: %v (%q)", err, cr.Body())
	}
	id, _ := created["id"].(string)
	if id == "" || !uuidRe.MatchString(id) {
		t.Fatalf("expected UUID id, got %v", created["id"])
	}
	if created["title"] != "Hello" {
		t.Fatalf("expected title=Hello, got %v", created["title"])
	}

	gr := mustGET(t, client, "http://test/api/articles/"+id)
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/articles/{id} expected 200, got %d", sc)
	}
	var gotByID map[string]any
	if err := json.Unmarshal(gr.Body(), &gotByID); err != nil {
		t.Fatalf("invalid JSON getById: %v (%q)", err, gr.Body())
	}
	if gotByID["id"] != id || gotByID["title"] != "Hello" {
		t.Fatalf("getById mismatch: %+v", gotByID)
	}

	ga := mustGET(t, client, "http://test/api/articles")
	if sc := ga.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/articles expected 200, got %d", sc)
	}
	var list []map[string]any
	if err := json.Unmarshal(ga.Body(), &list); err != nil {
		t.Fatalf("invalid JSON list: %v (%q)", err, ga.Body())
	}
	found := false
	for _, it := range list {
		if it["id"] == id {
			found = true
			break
		}
	}
	if !found {
		t.Fatalf("created id %s not found in GET all: %+v", id, list)
	}

	up := mustPUTJSON(t, client, "http://test/api/articles/"+id, map[string]any{
		"title": "Updated",
		"extra": 123,
	})
	if sc := up.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("PUT /api/articles/{id} expected 200, got %d", sc)
	}
	var updated map[string]any
	if err := json.Unmarshal(up.Body(), &updated); err != nil {
		t.Fatalf("invalid JSON update: %v (%q)", err, up.Body())
	}
	if updated["id"] != id || updated["title"] != "Updated" {
		t.Fatalf("update mismatch: %+v", updated)
	}
	gr2 := mustGET(t, client, "http://test/api/articles/"+id)
	var got2 map[string]any
	_ = json.Unmarshal(gr2.Body(), &got2)
	if got2["title"] != "Updated" || got2["extra"] != float64(123) /* JSON -> float64 */ {
		t.Fatalf("update not persisted: %+v", got2)
	}

	dr := mustDELETE(t, client, "http://test/api/articles/"+id)
	if sc := dr.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("DELETE /api/articles/{id} expected 204, got %d", sc)
	}
	gr3 := mustGET(t, client, "http://test/api/articles/"+id)
	if sc := gr3.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET deleted id expected 200 (null body), got %d", sc)
	}
	if string(gr3.Body()) != "null" {
		t.Fatalf("expected body null after delete, got %q", gr3.Body())
	}

	c1 := mustPOSTJSON(t, client, "http://test/api/comments", map[string]any{"body": "c1"})
	c2 := mustPOSTJSON(t, client, "http://test/api/comments", map[string]any{"body": "c2"})
	if c1.StatusCode() != 200 || c2.StatusCode() != 200 {
		t.Fatalf("POST /api/comments should be 200; got %d and %d", c1.StatusCode(), c2.StatusCode())
	}
	destroy := mustDELETE(t, client, "http://test/api/comments")
	if sc := destroy.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("DELETE /api/comments expected 204, got %d", sc)
	}
	gal := mustGET(t, client, "http://test/api/comments")
	if sc := gal.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/comments expected 200, got %d", sc)
	}
	var empty []map[string]any
	if err := json.Unmarshal(gal.Body(), &empty); err != nil {
		t.Fatalf("invalid JSON list after destroy: %v (%q)", err, gal.Body())
	}
	if len(empty) != 0 {
		t.Fatalf("expected empty list after destroy, got %v", empty)
	}
}

func TestAutoREST_WorksForArbitraryEntityNames(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	names := []string{"users", "orders", "weird-entity_name42"}
	for _, entity := range names {
		resp := mustPOSTJSON(t, client, "http://test/api/"+entity, map[string]any{
			"note": "auto",
		})
		if resp.StatusCode() != fasthttp.StatusOK {
			t.Fatalf("POST /api/%s expected 200, got %d", entity, resp.StatusCode())
		}
		var created map[string]any
		_ = json.Unmarshal(resp.Body(), &created)
		id, _ := created["id"].(string)
		if id == "" || !uuidRe.MatchString(id) {
			t.Fatalf("[%s] expected UUID id, got %v", entity, created["id"])
		}
		gr := mustGET(t, client, "http://test/api/"+entity+"/"+id)
		if gr.StatusCode() != fasthttp.StatusOK {
			t.Fatalf("GET /api/%s/%s expected 200, got %d", entity, id, gr.StatusCode())
		}
	}
}

func TestAutoREST_Filters(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/books", map[string]any{"title": "Go in Action", "author": "Alice"})
	mustPOSTJSON(t, client, "http://test/api/books", map[string]any{"title": "Learning Python", "author": "Bob"})
	mustPOSTJSON(t, client, "http://test/api/books", map[string]any{"title": "Advanced Go", "author": "Alice"})

	gr := mustGET(t, client, "http://test/api/books?filter[author]=Alice")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/books?filter[author]=Alice expected 200, got %d", sc)
	}
	var byAuthor []map[string]any
	if err := json.Unmarshal(gr.Body(), &byAuthor); err != nil {
		t.Fatalf("invalid JSON filter response: %v (%q)", err, gr.Body())
	}
	if len(byAuthor) != 2 {
		t.Fatalf("expected 2 books by Alice, got %d (%v)", len(byAuthor), byAuthor)
	}

	gr2 := mustGET(t, client, "http://test/api/books?filter[title]=Learning+Python")
	if sc := gr2.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/books?filter[title]=Learning Python expected 200, got %d", sc)
	}
	var byTitle []map[string]any
	if err := json.Unmarshal(gr2.Body(), &byTitle); err != nil {
		t.Fatalf("invalid JSON filter response: %v (%q)", err, gr2.Body())
	}
	if len(byTitle) != 1 || byTitle[0]["title"] != "Learning Python" {
		t.Fatalf("expected only Learning Python, got %v", byTitle)
	}
}

func TestAutoREST_CombinedFilters(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/movies", map[string]any{"title": "Inception", "director": "Nolan", "year": 2010})
	mustPOSTJSON(t, client, "http://test/api/movies", map[string]any{"title": "Interstellar", "director": "Nolan", "year": 2014})
	mustPOSTJSON(t, client, "http://test/api/movies", map[string]any{"title": "Dunkirk", "director": "Nolan", "year": 2017})
	mustPOSTJSON(t, client, "http://test/api/movies", map[string]any{"title": "The Matrix", "director": "Wachowski", "year": 1999})

	gr := mustGET(t, client, "http://test/api/movies?filter[director]=Nolan&filter[year]=2014")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/movies?filter[director]=Nolan&filter[year]=2014 expected 200, got %d", sc)
	}
	var filtered []map[string]any
	if err := json.Unmarshal(gr.Body(), &filtered); err != nil {
		t.Fatalf("invalid JSON filter response: %v (%q)", err, gr.Body())
	}
	if len(filtered) != 1 || filtered[0]["title"] != "Interstellar" {
		t.Fatalf("expected only Interstellar, got %v", filtered)
	}

	gr2 := mustGET(t, client, "http://test/api/movies?filter[director]=No*&filter[title]=Incep*")
	if sc := gr2.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/movies?filter[director]=No*&filter[title]=Incep* expected 200, got %d", sc)
	}
	var filtered2 []map[string]any
	if err := json.Unmarshal(gr2.Body(), &filtered2); err != nil {
		t.Fatalf("invalid JSON filter response: %v (%q)", err, gr2.Body())
	}
	if len(filtered2) != 1 || filtered2[0]["title"] != "Inception" {
		t.Fatalf("expected only Inception, got %v", filtered2)
	}
}

func TestAutoREST_FilterEqAndNeq(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/authors", map[string]any{"name": "Alice"})
	mustPOSTJSON(t, client, "http://test/api/authors", map[string]any{"name": "Bob"})
	mustPOSTJSON(t, client, "http://test/api/authors", map[string]any{"name": "Charlie"})

	gr := mustGET(t, client, "http://test/api/authors?filter[name][eq]=Alice")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/authors?filter[name][eq]=Alice expected 200, got %d", sc)
	}
	var eqResult []map[string]any
	if err := json.Unmarshal(gr.Body(), &eqResult); err != nil {
		t.Fatalf("invalid JSON filter eq response: %v (%q)", err, gr.Body())
	}
	if len(eqResult) != 1 || eqResult[0]["name"] != "Alice" {
		t.Fatalf("expected only Alice, got %v", eqResult)
	}

	gr2 := mustGET(t, client, "http://test/api/authors?filter[name][neq]=Alice")
	if sc := gr2.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/authors?filter[name][neq]=Alice expected 200, got %d", sc)
	}
	var neqResult []map[string]any
	if err := json.Unmarshal(gr2.Body(), &neqResult); err != nil {
		t.Fatalf("invalid JSON filter neq response: %v (%q)", err, gr2.Body())
	}
	if len(neqResult) != 2 {
		t.Fatalf("expected 2 results without Alice, got %d (%v)", len(neqResult), neqResult)
	}
	names := map[string]bool{}
	for _, r := range neqResult {
		names[r["name"].(string)] = true
	}
	if names["Alice"] {
		t.Fatalf("Alice should not be in neq results: %v", neqResult)
	}
}

func TestAutoREST_SortAscendingDescending(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/products", map[string]any{"name": "Banana", "price": 2})
	mustPOSTJSON(t, client, "http://test/api/products", map[string]any{"name": "Apple", "price": 1})
	mustPOSTJSON(t, client, "http://test/api/products", map[string]any{"name": "Cherry", "price": 3})

	grAsc := mustGET(t, client, "http://test/api/products?sort[price]=asc")
	if sc := grAsc.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/products?sort[price]=asc expected 200, got %d", sc)
	}
	var ascResult []map[string]any
	if err := json.Unmarshal(grAsc.Body(), &ascResult); err != nil {
		t.Fatalf("invalid JSON asc response: %v (%q)", err, grAsc.Body())
	}
	if len(ascResult) != 3 || ascResult[0]["name"] != "Apple" || ascResult[1]["name"] != "Banana" || ascResult[2]["name"] != "Cherry" {
		t.Fatalf("unexpected asc order: %v", ascResult)
	}

	grDesc := mustGET(t, client, "http://test/api/products?sort[price]=desc")
	if sc := grDesc.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/products?sort[price]=desc expected 200, got %d", sc)
	}
	var descResult []map[string]any
	if err := json.Unmarshal(grDesc.Body(), &descResult); err != nil {
		t.Fatalf("invalid JSON desc response: %v (%q)", err, grDesc.Body())
	}
	if len(descResult) != 3 || descResult[0]["name"] != "Cherry" || descResult[1]["name"] != "Banana" || descResult[2]["name"] != "Apple" {
		t.Fatalf("unexpected desc order: %v", descResult)
	}
}

func TestAutoREST_ArrayFilters_AllOps(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{"title": "Post1", "tags": []string{"go", "db", "fast"}})
	mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{"title": "Post2", "tags": []string{"go", "api"}})
	mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{"title": "Post3", "tags": []string{"cli", "tool"}})

	gr := mustGET(t, client, "http://test/api/articles?filter[tags][contains]=go")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/articles?filter[tags][contains]=go expected 200, got %d", sc)
	}
	var contains []map[string]interface{}
	_ = json.Unmarshal(gr.Body(), &contains)
	if len(contains) != 2 {
		t.Fatalf("expected 2 contains results, got %v", contains)
	}

	gr2 := mustGET(t, client, "http://test/api/articles?filter[tags][not_contains]=go")
	var notContains []map[string]interface{}
	_ = json.Unmarshal(gr2.Body(), &notContains)
	if len(notContains) != 1 || notContains[0]["title"] != "Post3" {
		t.Fatalf("expected not_contains Post3, got %v", notContains)
	}

	gr3 := mustGET(t, client, "http://test/api/articles?filter[tags][all]=go,db")
	var all []map[string]interface{}
	_ = json.Unmarshal(gr3.Body(), &all)
	if len(all) != 1 || all[0]["title"] != "Post1" {
		t.Fatalf("expected all Post1, got %v", all)
	}

	gr4 := mustGET(t, client, "http://test/api/articles?filter[tags][any]=db,tool")
	var any []map[string]interface{}
	_ = json.Unmarshal(gr4.Body(), &any)
	if len(any) != 2 {
		t.Fatalf("expected 2 any results, got %v", any)
	}

	gr5 := mustGET(t, client, "http://test/api/articles?filter[tags][none]=go,api")
	var none []map[string]interface{}
	_ = json.Unmarshal(gr5.Body(), &none)
	if len(none) != 1 || none[0]["title"] != "Post3" {
		t.Fatalf("expected none Post3, got %v", none)
	}

	gr6 := mustGET(t, client, "http://test/api/articles?filter[tags][eq]=go,db,fast")
	var eq []map[string]interface{}
	_ = json.Unmarshal(gr6.Body(), &eq)
	if len(eq) != 1 || eq[0]["title"] != "Post1" {
		t.Fatalf("expected eq Post1, got %v", eq)
	}
}

func TestAutoREST_ArrayFilters_Combined(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/projects", map[string]any{
		"name":  "Proj1",
		"tags":  []string{"go", "db"},
		"owner": "Alice",
	})
	mustPOSTJSON(t, client, "http://test/api/projects", map[string]any{
		"name":  "Proj2",
		"tags":  []string{"api", "db"},
		"owner": "Bob",
	})
	mustPOSTJSON(t, client, "http://test/api/projects", map[string]any{
		"name":  "Proj3",
		"tags":  []string{"cli", "tool"},
		"owner": "Alice",
	})

	gr := mustGET(t, client, "http://test/api/projects?filter[tags][contains]=db&filter[owner][eq]=Alice")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/projects combined filter expected 200, got %d", sc)
	}
	var res []map[string]any
	_ = json.Unmarshal(gr.Body(), &res)
	if len(res) != 1 || res[0]["name"] != "Proj1" {
		t.Fatalf("expected Proj1, got %v", res)
	}

	gr2 := mustGET(t, client, "http://test/api/projects?filter[tags][any]=cli,db&filter[owner][neq]=Bob")
	var res2 []map[string]any
	_ = json.Unmarshal(gr2.Body(), &res2)
	if len(res2) != 2 {
		t.Fatalf("expected 2 projects for combined any+neq, got %v", res2)
	}
}

func TestAutoREST_ArrayFilters_MixedTypes(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/items", map[string]interface{}{
		"name":  "Item1",
		"tags":  []interface{}{"1", 2, "three"},
		"price": 10,
	})
	mustPOSTJSON(t, client, "http://test/api/items", map[string]interface{}{
		"name":  "Item2",
		"tags":  []interface{}{"4", "five"},
		"price": 20,
	})

	gr := mustGET(t, client, "http://test/api/items?filter[tags][contains]=2")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/items?filter[tags][contains]=2 expected 200, got %d", sc)
	}
	var res []map[string]any
	_ = json.Unmarshal(gr.Body(), &res)
	if len(res) != 1 || res[0]["name"] != "Item1" {
		t.Fatalf("expected Item1, got %v", res)
	}

	gr2 := mustGET(t, client, "http://test/api/items?filter[tags][none]=2,three")
	var res2 []map[string]any
	_ = json.Unmarshal(gr2.Body(), &res2)
	if len(res2) != 1 || res2[0]["name"] != "Item2" {
		t.Fatalf("expected Item2 for none filter, got %v", res2)
	}
}

func TestAutoREST_ArrayFilters_Chained(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/posts", map[string]any{"title": "P1", "tags": []string{"go", "fast"}})
	mustPOSTJSON(t, client, "http://test/api/posts", map[string]any{"title": "P2", "tags": []string{"go"}})
	mustPOSTJSON(t, client, "http://test/api/posts", map[string]any{"title": "P3", "tags": []string{"fast", "cli"}})

	gr := mustGET(t, client, "http://test/api/posts?filter[tags][contains]=go&filter[tags][not_contains]=cli")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET chained filters expected 200, got %d", sc)
	}
	var res []map[string]any
	_ = json.Unmarshal(gr.Body(), &res)
	if len(res) != 2 {
		t.Fatalf("expected 2 posts for chained filters, got %v", res)
	}
}

func TestAutoREST_SchemaValidation_BasicAndNested(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	globals.SetConfig(cfg)

	resp1 := mustPOSTJSON(t, client, "http://test/api/users", map[string]any{
		"name": "Alice",
		"age":  30,
		"info": map[string]any{"city": "Paris", "zip": 75000},
	})
	if sc := resp1.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("POST /api/users expected 200, got %d", sc)
	}
	var created1 map[string]any
	_ = json.Unmarshal(resp1.Body(), &created1)
	if created1["id"] == "" {
		t.Fatalf("expected id in first creation, got %+v", created1)
	}

	resp2 := mustPOSTJSON(t, client, "http://test/api/users", map[string]any{
		"name": 123,
		"age":  40,
		"info": map[string]any{"city": "Paris", "zip": "not_a_number"},
	})
	if sc := resp2.StatusCode(); sc == fasthttp.StatusOK {
		t.Fatalf("expected validation error for type mismatch, got 200")
	}

	resp3 := mustPOSTJSON(t, client, "http://test/api/users", map[string]any{
		"name": "Bob",
		"age":  22,
		"info": map[string]any{"city": "Lyon", "zip": 69000},
	})
	if sc := resp3.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200 for valid schema, got %d", sc)
	}
	var created3 map[string]any
	_ = json.Unmarshal(resp3.Body(), &created3)
	if created3["name"] != "Bob" {
		t.Fatalf("unexpected created3 %+v", created3)
	}
}

func TestAutoREST_SchemaValidation_NewEntity_AcceptsDifferentSchemas(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	globals.SetConfig(cfg)

	r1 := mustPOSTJSON(t, client, "http://test/api/orders", map[string]any{
		"ref":   "ORD001",
		"total": 123.45,
	})
	if sc := r1.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("POST /api/orders expected 200, got %d", sc)
	}

	r2 := mustPOSTJSON(t, client, "http://test/api/customers", map[string]any{
		"name": "Alice",
		"vip":  true,
	})
	if sc := r2.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("POST /api/customers expected 200, got %d", sc)
	}
}

func TestAutoREST_SchemaValidation_DeepNested(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	globals.SetConfig(cfg)

	r1 := mustPOSTJSON(t, client, "http://test/api/projects", map[string]any{
		"title": "Test",
		"meta": map[string]any{
			"author": map[string]any{
				"name": "Alice",
				"age":  30,
			},
		},
	})
	if sc := r1.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}

	r2 := mustPOSTJSON(t, client, "http://test/api/projects", map[string]any{
		"title": "WrongType",
		"meta": map[string]any{
			"author": map[string]any{
				"name": 999,
				"age":  "old",
			},
		},
	})
	if sc := r2.StatusCode(); sc == fasthttp.StatusOK {
		t.Fatalf("expected validation error for deep nested type mismatch, got 200")
	}
}

func TestAutoREST_CreateAndUpdate_WithSubEntities(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	resp := mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "Nested Example",
		"author": map[string]any{
			"@entity":  "author",
			"fullname": "Taymour Negib",
			"job": map[string]any{
				"@entity":     "job",
				"designation": "Writer",
			},
		},
	})
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}

	var article map[string]any
	if err := json.Unmarshal(resp.Body(), &article); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}

	aAuthor, ok := article["author"].(map[string]any)
	if !ok || aAuthor["@entity"] != "author" {
		t.Fatalf("expected author sub-entity link, got %+v", article)
	}
	authorID, _ := aAuthor["id"].(string)
	if authorID == "" || !uuidRe.MatchString(authorID) {
		t.Fatalf("invalid author id: %v", authorID)
	}

	grAuthor := mustGET(t, client, "http://test/api/author/"+authorID)
	var author map[string]any
	_ = json.Unmarshal(grAuthor.Body(), &author)
	if author["fullname"] != "Taymour Negib" {
		t.Fatalf("expected fullname Taymour Negib, got %+v", author)
	}
	job, ok := author["job"].(map[string]any)
	if !ok || job["@entity"] != "job" {
		t.Fatalf("expected job reference in author, got %+v", author)
	}
	jobID := job["id"].(string)
	grJob := mustGET(t, client, "http://test/api/job/"+jobID)
	var jobObj map[string]any
	_ = json.Unmarshal(grJob.Body(), &jobObj)
	if jobObj["designation"] != "Writer" {
		t.Fatalf("expected Writer, got %+v", jobObj)
	}

	up := mustPUTJSON(t, client, "http://test/api/articles/"+article["id"].(string), map[string]any{
		"title": "Nested Updated",
		"author": map[string]any{
			"@entity":  "author",
			"id":       authorID,
			"fullname": "Updated Author",
			"job": map[string]any{
				"@entity":     "job",
				"id":          jobID,
				"designation": "Editor",
			},
		},
	})
	if sc := up.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200 on update, got %d", sc)
	}

	grAuthor2 := mustGET(t, client, "http://test/api/author/"+authorID)
	var author2 map[string]any
	_ = json.Unmarshal(grAuthor2.Body(), &author2)
	if author2["fullname"] != "Updated Author" {
		t.Fatalf("expected Updated Author, got %+v", author2)
	}
	job2 := author2["job"].(map[string]any)
	grJob2 := mustGET(t, client, "http://test/api/job/"+job2["id"].(string))
	var jobObj2 map[string]any
	_ = json.Unmarshal(grJob2.Body(), &jobObj2)
	if jobObj2["designation"] != "Editor" {
		t.Fatalf("expected job designation Editor, got %+v", jobObj2)
	}
}

func TestAutoREST_Create_WithArraySubEntities(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	resp := mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "Multi Author Article",
		"authors": []map[string]any{
			{
				"@entity":  "author",
				"fullname": "Alice",
				"job": map[string]any{
					"@entity":     "job",
					"designation": "Writer",
				},
			},
			{
				"@entity":  "author",
				"fullname": "Bob",
			},
		},
	})
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}
	var article map[string]any
	_ = json.Unmarshal(resp.Body(), &article)
	authors, ok := article["authors"].([]interface{})
	if !ok || len(authors) != 2 {
		t.Fatalf("expected 2 authors, got %+v", article)
	}
	for _, a := range authors {
		am := a.(map[string]any)
		if am["@entity"] != "author" {
			t.Fatalf("expected @entity=author, got %+v", am)
		}
		id, _ := am["id"].(string)
		if id == "" || !uuidRe.MatchString(id) {
			t.Fatalf("invalid author id %+v", am)
		}
		gr := mustGET(t, client, "http://test/api/author/"+id)
		if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
			t.Fatalf("GET /api/author/%s expected 200, got %d", id, sc)
		}
	}
}

func TestAutoREST_Update_WithArraySubEntities(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	create := mustPOSTJSON(t, client, "http://test/api/albums", map[string]any{
		"title": "Album 1",
		"tracks": []map[string]any{
			{"@entity": "track", "title": "Track 1"},
			{"@entity": "track", "title": "Track 2"},
		},
	})
	var album map[string]any
	_ = json.Unmarshal(create.Body(), &album)
	tracks := album["tracks"].([]interface{})
	first := tracks[0].(map[string]any)
	second := tracks[1].(map[string]any)
	firstID := first["id"].(string)
	secondID := second["id"].(string)

	update := mustPUTJSON(t, client, "http://test/api/albums/"+album["id"].(string), map[string]any{
		"title": "Album Updated",
		"tracks": []map[string]any{
			{"@entity": "track", "id": firstID, "title": "Track 1 Updated"},
			{"@entity": "track", "id": secondID, "title": "Track 2 Updated"},
			{"@entity": "track", "title": "Track 3 New"},
		},
	})
	if sc := update.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}
	var updated map[string]any
	_ = json.Unmarshal(update.Body(), &updated)
	newTracks := updated["tracks"].([]interface{})
	if len(newTracks) != 3 {
		t.Fatalf("expected 3 tracks after update, got %+v", updated)
	}
}

func TestAutoREST_Includes_SimpleAndNested(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	jobResp := mustPOSTJSON(t, client, "http://test/api/job", map[string]any{
		"designation": "Writer",
	})
	var job map[string]any
	_ = json.Unmarshal(jobResp.Body(), &job)
	jobID := job["id"].(string)

	authorResp := mustPOSTJSON(t, client, "http://test/api/author", map[string]any{
		"fullname": "Alice",
		"job": map[string]any{
			"@entity": "job",
			"id":      jobID,
		},
	})
	var author map[string]any
	_ = json.Unmarshal(authorResp.Body(), &author)
	authorID := author["id"].(string)

	categoryResp := mustPOSTJSON(t, client, "http://test/api/category", map[string]any{
		"type": "tech",
	})
	var category map[string]any
	_ = json.Unmarshal(categoryResp.Body(), &category)
	categoryID := category["id"].(string)

	mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "Article A",
		"category": map[string]any{
			"@entity": "category",
			"id":      categoryID,
		},
		"author": map[string]any{
			"@entity": "author",
			"id":      authorID,
		},
	})

	gr := mustGET(t, client, "http://test/api/articles?includes=category,author,author.job")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/articles?includes expected 200, got %d", sc)
	}
	var list []map[string]any
	_ = json.Unmarshal(gr.Body(), &list)
	if len(list) != 1 {
		t.Fatalf("expected 1 article, got %v", list)
	}
	cat := list[0]["category"].(map[string]any)
	if cat["type"] != "tech" {
		t.Fatalf("expected category type tech, got %v", cat)
	}
	auth := list[0]["author"].(map[string]any)
	if auth["fullname"] != "Alice" {
		t.Fatalf("expected author Alice, got %v", auth)
	}
	jobIn := auth["job"].(map[string]any)
	if jobIn["designation"] != "Writer" {
		t.Fatalf("expected job Writer, got %v", jobIn)
	}
}

func TestAutoREST_Includes_AllMode(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	jobResp := mustPOSTJSON(t, client, "http://test/api/job", map[string]any{
		"designation": "Engineer",
	})
	var job map[string]any
	_ = json.Unmarshal(jobResp.Body(), &job)
	jobID := job["id"].(string)

	authorResp := mustPOSTJSON(t, client, "http://test/api/author", map[string]any{
		"fullname": "Bob",
		"job": map[string]any{
			"@entity": "job",
			"id":      jobID,
		},
	})
	var author map[string]any
	_ = json.Unmarshal(authorResp.Body(), &author)
	authorID := author["id"].(string)

	categoryResp := mustPOSTJSON(t, client, "http://test/api/category", map[string]any{
		"type": "science",
	})
	var category map[string]any
	_ = json.Unmarshal(categoryResp.Body(), &category)
	categoryID := category["id"].(string)

	mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "Full Mode Article",
		"category": map[string]any{
			"@entity": "category",
			"id":      categoryID,
		},
		"author": map[string]any{
			"@entity": "author",
			"id":      authorID,
		},
	})

	gr := mustGET(t, client, "http://test/api/articles?includes=all")
	var list []map[string]any
	_ = json.Unmarshal(gr.Body(), &list)
	if len(list) != 1 {
		t.Fatalf("expected 1 article, got %v", list)
	}
	cat := list[0]["category"].(map[string]any)
	if cat["type"] != "science" {
		t.Fatalf("expected category science, got %v", cat)
	}
	auth := list[0]["author"].(map[string]any)
	if auth["fullname"] != "Bob" {
		t.Fatalf("expected author Bob, got %v", auth)
	}
	jobIn := auth["job"].(map[string]any)
	if jobIn["designation"] != "Engineer" {
		t.Fatalf("expected job Engineer, got %v", jobIn)
	}
}

func TestAutoREST_FiltersOnIncludedEntities(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	jobResp := mustPOSTJSON(t, client, "http://test/api/job", map[string]any{"designation": "Dev"})
	var job map[string]any
	_ = json.Unmarshal(jobResp.Body(), &job)
	jobID := job["id"].(string)

	auth1 := mustPOSTJSON(t, client, "http://test/api/author", map[string]any{
		"fullname": "Alice",
		"job": map[string]any{
			"@entity": "job",
			"id":      jobID,
		},
	})
	var a1 map[string]any
	_ = json.Unmarshal(auth1.Body(), &a1)
	a1id := a1["id"].(string)

	auth2 := mustPOSTJSON(t, client, "http://test/api/author", map[string]any{
		"fullname": "Bob",
	})
	var a2 map[string]any
	_ = json.Unmarshal(auth2.Body(), &a2)
	a2id := a2["id"].(string)

	mustPOSTJSON(t, client, "http://test/api/posts", map[string]any{
		"title": "P1",
		"author": map[string]any{
			"@entity": "author",
			"id":      a1id,
		},
	})
	mustPOSTJSON(t, client, "http://test/api/posts", map[string]any{
		"title": "P2",
		"author": map[string]any{
			"@entity": "author",
			"id":      a2id,
		},
	})

	gr := mustGET(t, client, "http://test/api/posts?includes=author,author.job&filter[author.fullname][eq]=Alice")
	var list []map[string]any
	_ = json.Unmarshal(gr.Body(), &list)
	if len(list) != 1 || list[0]["title"] != "P1" {
		t.Fatalf("expected P1 filtered by author Alice, got %v", list)
	}
	auth := list[0]["author"].(map[string]any)
	jobIn := auth["job"].(map[string]any)
	if jobIn["designation"] != "Dev" {
		t.Fatalf("expected included job Dev, got %v", jobIn)
	}
}

func TestAutoREST_FiltersOnIncludedNested(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	jobResp := mustPOSTJSON(t, client, "http://test/api/job", map[string]any{"designation": "Designer"})
	var job map[string]any
	_ = json.Unmarshal(jobResp.Body(), &job)
	jobID := job["id"].(string)

	auth := mustPOSTJSON(t, client, "http://test/api/author", map[string]any{
		"fullname": "Claire",
		"job": map[string]any{
			"@entity": "job",
			"id":      jobID,
		},
	})
	var a map[string]any
	_ = json.Unmarshal(auth.Body(), &a)
	aID := a["id"].(string)

	mustPOSTJSON(t, client, "http://test/api/posts", map[string]any{
		"title": "With Designer",
		"author": map[string]any{
			"@entity": "author",
			"id":      aID,
		},
	})
	mustPOSTJSON(t, client, "http://test/api/posts", map[string]any{
		"title": "Without Designer",
	})

	gr := mustGET(t, client, "http://test/api/posts?includes=author,author.job&filter[author.job.designation][eq]=Designer")
	var list []map[string]any
	_ = json.Unmarshal(gr.Body(), &list)
	if len(list) != 1 || list[0]["title"] != "With Designer" {
		t.Fatalf("expected With Designer, got %v", list)
	}
}

func TestAutoREST_GetByID_WithIncludes(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	jobResp := mustPOSTJSON(t, client, "http://test/api/job", map[string]any{"designation": "Writer"})
	var job map[string]any
	_ = json.Unmarshal(jobResp.Body(), &job)
	jobID := job["id"].(string)

	authorResp := mustPOSTJSON(t, client, "http://test/api/author", map[string]any{
		"fullname": "Alice",
		"job": map[string]any{
			"@entity": "job",
			"id":      jobID,
		},
	})
	var author map[string]any
	_ = json.Unmarshal(authorResp.Body(), &author)
	authorID := author["id"].(string)

	categoryResp := mustPOSTJSON(t, client, "http://test/api/category", map[string]any{"type": "tech"})
	var category map[string]any
	_ = json.Unmarshal(categoryResp.Body(), &category)
	categoryID := category["id"].(string)

	articleResp := mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "Nested Article",
		"category": map[string]any{
			"@entity": "category",
			"id":      categoryID,
		},
		"author": map[string]any{
			"@entity": "author",
			"id":      authorID,
		},
	})
	var article map[string]any
	_ = json.Unmarshal(articleResp.Body(), &article)
	articleID := article["id"].(string)

	gr := mustGET(t, client, "http://test/api/articles/"+articleID+"?includes=category,author,author.job")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/articles/{id}?includes expected 200, got %d", sc)
	}
	var got map[string]any
	_ = json.Unmarshal(gr.Body(), &got)
	cat := got["category"].(map[string]any)
	if cat["type"] != "tech" {
		t.Fatalf("expected category tech, got %v", cat)
	}
	auth := got["author"].(map[string]any)
	if auth["fullname"] != "Alice" {
		t.Fatalf("expected author Alice, got %v", auth)
	}
	jobIn := auth["job"].(map[string]any)
	if jobIn["designation"] != "Writer" {
		t.Fatalf("expected job Writer, got %v", jobIn)
	}
}

func TestAutoREST_GetByID_WithIncludesAll(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	jobResp := mustPOSTJSON(t, client, "http://test/api/job", map[string]any{"designation": "Engineer"})
	var job map[string]any
	_ = json.Unmarshal(jobResp.Body(), &job)
	jobID := job["id"].(string)

	authorResp := mustPOSTJSON(t, client, "http://test/api/author", map[string]any{
		"fullname": "Bob",
		"job": map[string]any{
			"@entity": "job",
			"id":      jobID,
		},
	})
	var author map[string]any
	_ = json.Unmarshal(authorResp.Body(), &author)
	authorID := author["id"].(string)

	categoryResp := mustPOSTJSON(t, client, "http://test/api/category", map[string]any{"type": "science"})
	var category map[string]any
	_ = json.Unmarshal(categoryResp.Body(), &category)
	categoryID := category["id"].(string)

	articleResp := mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "All Mode Article",
		"category": map[string]any{
			"@entity": "category",
			"id":      categoryID,
		},
		"author": map[string]any{
			"@entity": "author",
			"id":      authorID,
		},
	})
	var article map[string]any
	_ = json.Unmarshal(articleResp.Body(), &article)
	articleID := article["id"].(string)

	gr := mustGET(t, client, "http://test/api/articles/"+articleID+"?includes=all")
	if sc := gr.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("GET /api/articles/{id}?includes=all expected 200, got %d", sc)
	}
	var got map[string]any
	_ = json.Unmarshal(gr.Body(), &got)
	cat := got["category"].(map[string]any)
	if cat["type"] != "science" {
		t.Fatalf("expected category science, got %v", cat)
	}
	auth := got["author"].(map[string]any)
	if auth["fullname"] != "Bob" {
		t.Fatalf("expected author Bob, got %v", auth)
	}
	jobIn := auth["job"].(map[string]any)
	if jobIn["designation"] != "Engineer" {
		t.Fatalf("expected job Engineer, got %v", jobIn)
	}
}

func TestMigrations_SetAction(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	create := mustPOSTJSON(t, client, "http://test/api/users", map[string]any{
		"name": "OldName",
		"age":  25,
	})
	if create.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", create.StatusCode())
	}

	body := []map[string]any{
		{
			"set": []map[string]any{
				{"name": "NewName"},
			},
		},
	}

	resp := mustPOSTJSON(t, client, "http://test/api/users/migrate", body)
	if resp.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("migration expected 200, got %d (%s)", resp.StatusCode(), resp.Body())
	}

	gr := mustGET(t, client, "http://test/api/users")
	if gr.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("GET users expected 200, got %d", gr.StatusCode())
	}

	var users []map[string]any
	_ = json.Unmarshal(gr.Body(), &users)
	if len(users) == 0 {
		t.Fatalf("no users found after migration")
	}
	for _, u := range users {
		if u["name"] != "NewName" {
			t.Fatalf("expected name NewName, got %v", u["name"])
		}
	}
}

func TestMigrations_SetNestedField(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	mustPOSTJSON(t, client, "http://test/api/accounts", map[string]any{
		"username": "test",
		"profile": map[string]any{
			"city": "Paris",
		},
	})

	body := []map[string]any{
		{
			"set": []map[string]any{
				{"profile.city": "Lyon"},
			},
		},
	}

	resp := mustPOSTJSON(t, client, "http://test/api/accounts/migrate", body)
	if resp.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode())
	}

	gr := mustGET(t, client, "http://test/api/accounts")
	var accounts []map[string]any
	_ = json.Unmarshal(gr.Body(), &accounts)
	if len(accounts) == 0 {
		t.Fatalf("no accounts found after migration")
	}
	profile, _ := accounts[0]["profile"].(map[string]any)
	if profile["city"] != "Lyon" {
		t.Fatalf("expected profile.city Lyon, got %v", profile["city"])
	}
}
