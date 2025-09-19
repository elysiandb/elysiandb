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
}
