package e2e

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func TestHealthDoesNotRequireCredentials(t *testing.T) {
	for _, mode := range []string{"user", "token", "basic"} {
		t.Run(mode, func(t *testing.T) {
			s := startAPIServer(t, func(cfg *configuration.Config) {
				cfg.Security.Authentication.Mode = mode
			})

			resp := s.mustDo(fasthttp.StatusOK, "GET", "/health", "")
			if got := resp.header("X-Elysian-Version"); got != globals.VERSION {
				t.Fatalf("expected X-Elysian-Version %q, got %q", globals.VERSION, got)
			}

			s.mustDo(fasthttp.StatusUnauthorized, "GET", "/api/book", "")
		})
	}
}

func TestVersionHeaderReportsBuildVersion(t *testing.T) {
	original := globals.VERSION
	globals.VERSION = "1.2.3-test"
	defer func() { globals.VERSION = original }()

	s := startAPIServer(t, nil)

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/health", "").header("X-Elysian-Version"); got != "1.2.3-test" {
		t.Fatalf("expected the injected version, got %q", got)
	}
}

func TestHookUpdateKeepsOmittedFields(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/hk", `{"id":"v1","n":1}`)
	created := s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/hk", `{"name":"tag","event":"post_read","language":"javascript","priority":5,"enabled":true,"script":"function postRead(ctx) { ctx.entity.tagged = true }"}`).object(t)
	hookID := created["id"].(string)

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/hk/v1", "").object(t)["tagged"]; got != true {
		t.Fatalf("expected the hook to run before the update, got %v", got)
	}

	updated := s.mustDo(fasthttp.StatusOK, "PUT", "/api/hook/id/"+hookID, `{"script":"function postRead(ctx) { ctx.entity.tagged = 2 }"}`).object(t)
	want := map[string]any{"name": "tag", "entity": "hk", "event": "post_read", "language": "javascript", "priority": float64(5), "enabled": true}
	for field, value := range want {
		if updated[field] != value {
			t.Fatalf("expected %s=%v after a partial update, got %v (%v)", field, value, updated[field], updated)
		}
	}

	if hooks := s.mustDo(fasthttp.StatusOK, "GET", "/api/hook/hk", "").list(t); len(hooks) != 1 {
		t.Fatalf("expected the hook to stay attached to its entity, got %v", hooks)
	}

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/hk/v1", "").object(t)["tagged"]; got != float64(2) {
		t.Fatalf("expected the updated script to run, got %v", got)
	}

	s.mustDo(fasthttp.StatusNotFound, "PUT", "/api/hook/id/does-not-exist", `{"name":"x"}`)
	s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/hook/id/"+hookID, `{"name":""}`)

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/hook/id/"+hookID, "").object(t)["name"]; got != "tag" {
		t.Fatalf("a rejected update must not change the hook, got name %v", got)
	}
}

func TestHookRoutesWithAuthenticationDisabled(t *testing.T) {
	s := startAPIServer(t, func(cfg *configuration.Config) {
		cfg.Security.Authentication.Enabled = false
	})

	s.mustDo(fasthttp.StatusOK, "GET", "/api/movie", "")
	s.mustDo(fasthttp.StatusOK, "GET", "/api/hook/movie", "")
	created := s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/movie", `{"name":"h","event":"post_read","language":"javascript","script":"function postRead(ctx){ ctx.entity.seen = true }","enabled":true}`).object(t)
	s.mustDo(fasthttp.StatusOK, "GET", "/api/hook/id/"+created["id"].(string), "")
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/hook/id/"+created["id"].(string), `{"priority":3}`)
}

func TestHookRoutesInBasicMode(t *testing.T) {
	s := startAPIServer(t, func(cfg *configuration.Config) {
		cfg.Security.Authentication.Mode = "basic"
	})

	s.mustDo(fasthttp.StatusUnauthorized, "GET", "/api/hook/movie", "")

	auth := "Basic " + base64.StdEncoding.EncodeToString([]byte("admin:admin"))
	s.mustDo(fasthttp.StatusOK, "GET", "/api/hook/movie", "", "Authorization", auth)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/movie", `{"name":"h","event":"post_read","language":"javascript","script":"function postRead(ctx){}","enabled":true}`, "Authorization", auth)

	wrong := "Basic " + base64.StdEncoding.EncodeToString([]byte("admin:wrong"))
	s.mustDo(fasthttp.StatusUnauthorized, "GET", "/api/hook/movie", "", "Authorization", wrong)
}

func TestStrictManualSchemaPartialUpdate(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/film/create", `{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/film", `{"id":"f1","title":"C","year":1983}`)

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/film/f1", `{"year":1984}`)
	film := s.mustDo(fasthttp.StatusOK, "GET", "/api/film/f1", "").object(t)
	if film["title"] != "C" || film["year"] != float64(1984) {
		t.Fatalf("expected a partial update to keep the title, got %v", film)
	}

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/film/f1", `{"title":"C2","year":null}`)
	film = s.mustDo(fasthttp.StatusOK, "GET", "/api/film/f1", "").object(t)
	if value, ok := film["year"]; !ok || value != nil || film["title"] != "C2" {
		t.Fatalf("expected year to be unset with null, got %v", film)
	}

	resp := s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/film/f1", `{"title":null}`)
	if !strings.Contains(string(resp.body), "required field cannot be null") {
		t.Fatalf("expected a null required field to be rejected, got %s", resp.body)
	}

	resp = s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/film/f1", `{"rating":5}`)
	if !strings.Contains(string(resp.body), "field not allowed by strict schema") {
		t.Fatalf("expected an unknown field to be rejected, got %s", resp.body)
	}

	resp = s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/film/f1", `{"year":"1999"}`)
	if !strings.Contains(string(resp.body), "expected type number") {
		t.Fatalf("expected a wrong type to be rejected, got %s", resp.body)
	}

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/film", `[{"id":"f1","year":2001}]`)
	resp = s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/film", `[{"id":"f1","year":"x"}]`)
	if !strings.Contains(string(resp.body), "expected type number") {
		t.Fatalf("expected an invalid batch update to be rejected, got %s", resp.body)
	}
	if film = s.mustDo(fasthttp.StatusOK, "GET", "/api/film/f1", "").object(t); film["year"] != float64(2001) || film["title"] != "C2" {
		t.Fatalf("expected a partial batch update to keep the title, got %v", film)
	}

	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/film", `{"id":"f2","year":1990}`)
}

func TestStrictManualSchemaUpdateIgnoresUntouchedStoredFields(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/doc/create", `{"fields":{"title":{"type":"string","required":true},"legacy":{"type":"string"}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/doc", `{"id":"d1","title":"T","legacy":"old"}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/doc/schema", `{"fields":{"title":{"type":"string","required":true}}}`)

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/doc/d1", `{"title":"T2"}`)
	s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/doc/d1", `{"legacy":"new"}`)
}

func TestInferredSchemaIsExtendedNotReplaced(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/inf", `{"id":"i1","a":"x","b":1}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/inf", `{"id":"i2","c":true}`)

	fields := s.mustDo(fasthttp.StatusOK, "GET", "/api/inf/schema", "").object(t)["fields"].(map[string]any)
	for name, typ := range map[string]string{"a": "string", "b": "number", "c": "boolean"} {
		field, ok := fields[name].(map[string]any)
		if !ok || field["type"] != typ {
			t.Fatalf("expected field %s of type %s in the inferred schema, got %v", name, typ, fields)
		}
	}

	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/inf", `{"id":"i3","a":5}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/inf", `{"id":"i4","d":[1,2]}`)
}

func TestInferredSchemaAcceptsNullAndLaterTypes(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/nul", `{"id":"n1","x":null,"y":"s"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/nul", `{"id":"n2","x":"a","y":null}`)
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/nul", `{"id":"n3","x":5}`)
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/nul", `{"id":"n4","y":5}`)
}

func TestShorthandEntityTypeCreation(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/bk/create", `{"fields":{"title":"string","pages":"number"}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/bk", `{"title":"Dune","pages":700}`)
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/bk", `{"title":5}`)
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/bk", `{"title":"Dune","author":"x"}`)
}

func beginTransaction(t *testing.T, s *apiServer) string {
	t.Helper()

	return s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/begin", "").object(t)["transaction_id"].(string)
}

func TestFailedCommitAppliesNoOperation(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/acct", `{"id":"B","balance":100}`)
	tx := beginTransaction(t, s)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/acct", `{"id":"A","balance":50}`)
	s.mustDo(fasthttp.StatusOK, "DELETE", "/api/tx/"+tx+"/entity/acct/B", "")
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/tx/"+tx+"/entity/acct/B", `{"balance":0}`)

	resp := s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/tx/"+tx+"/commit", "")
	if !strings.Contains(string(resp.body), "update failed") {
		t.Fatalf("expected the commit to report the failing update, got %s", resp.body)
	}

	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/acct/A", "")
	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/acct/B", "").object(t)["balance"]; got != float64(100) {
		t.Fatalf("expected B to be left untouched, got balance %v", got)
	}
}

func TestCommitFailsWhenTargetIsDeletedBeforeCommit(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/acct", `{"id":"B","balance":100}`)
	tx := beginTransaction(t, s)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/acct", `{"id":"A","balance":50}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/tx/"+tx+"/entity/acct/B", `{"balance":0}`)
	s.mustDo(fasthttp.StatusNoContent, "DELETE", "/api/acct/B", "")

	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/tx/"+tx+"/commit", "")
	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/acct/A", "")
}

func TestCommitValidationFailureAppliesNoOperation(t *testing.T) {
	s := startAdminSession(t, nil)

	tx := beginTransaction(t, s)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/val", `{"id":"x1","a":"s"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/val", `{"id":"x2","a":5}`)

	resp := s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/tx/"+tx+"/commit", "")
	if !strings.Contains(string(resp.body), "validation error") {
		t.Fatalf("expected a validation error, got %s", resp.body)
	}

	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/val/x1", "")
}

func TestCommitAppliesOperationsThatDependOnEachOther(t *testing.T) {
	s := startAdminSession(t, nil)

	tx := beginTransaction(t, s)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/post", `{"id":"p1","title":"T","author":{"@entity":"writer","id":"w1","name":"Old"}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/post", `{"id":"p2","title":"U"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/commit", "")

	tx = beginTransaction(t, s)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/post", `{"id":"p3","title":"V","author":{"@entity":"writer","id":"w2","name":"New"}}`)
	s.mustDo(fasthttp.StatusOK, "DELETE", "/api/tx/"+tx+"/entity/post/p2", "")
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/post", `{"id":"p2","title":"U2"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/commit", "")

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/writer/w2", "").object(t)["name"]; got != "New" {
		t.Fatalf("expected the nested writer to be created, got %v", got)
	}

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/post/p2", "").object(t)["title"]; got != "U2" {
		t.Fatalf("expected p2 to be rewritten after its delete, got %v", got)
	}
}

func TestQueryCacheKeyIncludesCountOnlyAndFields(t *testing.T) {
	s := startAdminSession(t, nil)

	for i := 1; i <= 3; i++ {
		s.mustDo(fasthttp.StatusOK, "POST", "/api/book", fmt.Sprintf(`{"id":"b%d","title":"Book %d","pages":%d}`, i, i, i*100))
	}

	s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"book"}`)

	count := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"book","countOnly":true}`)
	if string(count.body) != `{"count":3}` || count.header("X-Elysian-Cache") != "MISS" {
		t.Fatalf("expected a fresh count, got %s (%s)", count.body, count.header("X-Elysian-Cache"))
	}

	projected := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"book","fields":"title"}`).list(t)
	for _, item := range projected {
		if _, ok := item["pages"]; ok || len(item) != 1 {
			t.Fatalf("expected only the projected field, got %v", item)
		}
	}

	s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"book","limit":50,"countOnly":true}`)
	if list := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"book","limit":50}`).list(t); len(list) != 3 {
		t.Fatalf("expected the document list, got %v", list)
	}

	if hit := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"book","countOnly":true}`); hit.header("X-Elysian-Cache") != "HIT" || string(hit.body) != `{"count":3}` {
		t.Fatalf("expected the count to be served from its own cache entry, got %s (%s)", hit.body, hit.header("X-Elysian-Cache"))
	}
}

func TestSearchMatchesBeforePaginationAndCombinesWithFilters(t *testing.T) {
	s := startAdminSession(t, nil)

	for i := 1; i <= 4; i++ {
		s.mustDo(fasthttp.StatusOK, "POST", "/api/note", fmt.Sprintf(`{"id":"n%d","title":"Note %d","kind":"k%d"}`, i, i, i%2))
	}
	s.mustDo(fasthttp.StatusOK, "POST", "/api/note", `{"id":"n5","title":"Zebra crossing","kind":"k1"}`)

	for _, path := range []string{"/api/note?search=*Zebra*", "/api/note?search=*Zebra*&limit=2", "/api/note?search=Zebra&limit=2", "/api/note?search=zebra"} {
		if ids := s.mustDo(fasthttp.StatusOK, "GET", path, "").ids(t); !reflect.DeepEqual(ids, []string{"n5"}) {
			t.Fatalf("GET %s: expected [n5], got %v", path, ids)
		}
	}

	ids := s.mustDo(fasthttp.StatusOK, "GET", "/api/note?search=Note&filter%5Bkind%5D%5Beq%5D=k0", "").ids(t)
	if !reflect.DeepEqual(sortedCopy(ids), []string{"n2", "n4"}) {
		t.Fatalf("expected search and filter to combine, got %v", ids)
	}

	if ids := s.mustDo(fasthttp.StatusOK, "GET", "/api/note?search=Note&limit=2&offset=3", "").ids(t); len(ids) != 1 {
		t.Fatalf("expected pagination over the 4 matches, got %v", ids)
	}

	if body := s.mustDo(fasthttp.StatusOK, "GET", "/api/note?search=Note&countOnly=true", "").body; string(body) != `{"count":4}` {
		t.Fatalf("expected 4 matches, got %s", body)
	}
}

func TestSearchIgnoresOwnerMarkersAndIds(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/movie", `{"id":"m1","title":"Alien"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/director", `{"id":"ridley-scott","name":"Ridley Scott"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/film", `{"id":"f1","title":"Blade Runner","directors":[{"@entity":"director","id":"ridley-scott"}]}`)

	for _, path := range []string{"/api/movie?search=admin", "/api/film?search=scott", "/api/film?search=director", "/api/movie?search=m1", "/api/film?search=admin&includes=directors"} {
		if ids := s.mustDo(fasthttp.StatusOK, "GET", path, "").ids(t); len(ids) != 0 {
			t.Fatalf("GET %s: expected no match, got %v", path, ids)
		}
	}

	for path, want := range map[string]string{"/api/movie?search=ali": "m1", "/api/film?search=runner": "f1", "/api/film?search=scott&includes=directors": "f1"} {
		if ids := s.mustDo(fasthttp.StatusOK, "GET", path, "").ids(t); !reflect.DeepEqual(ids, []string{want}) {
			t.Fatalf("GET %s: expected [%s], got %v", path, want, ids)
		}
	}
}

func TestUpdateOnMissingDocumentIsNotFound(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/book/create", `{"fields":{"title":{"type":"string","required":true},"year":{"type":"number","required":false}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"id":"b1","title":"Dune","year":1965}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/movie", `{"id":"m1","title":"Alien"}`)

	for path, body := range map[string]string{"/api/book/nope": `{"year":2000}`, "/api/movie/nope": `{"title":"x"}`} {
		if resp := s.mustDo(fasthttp.StatusNotFound, "PUT", path, body); string(resp.body) != `{"error":"entity not found"}` {
			t.Fatalf("PUT %s: expected an entity not found error, got %s", path, resp.body)
		}
	}

	resp := s.mustDo(fasthttp.StatusNotFound, "PUT", "/api/movie", `[{"id":"m1","title":"Alien (1979)"},{"id":"nope","title":"x"}]`).object(t)
	if !reflect.DeepEqual(resp["ids"], []any{"nope"}) {
		t.Fatalf("expected the missing ids to be listed, got %v", resp)
	}

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/movie/m1", "").object(t)["title"]; got != "Alien" {
		t.Fatalf("a batch with a missing id must not apply any update, got title %v", got)
	}

	if body := s.mustDo(fasthttp.StatusOK, "GET", "/api/movie/count", "").body; string(body) != `{"count":1}` {
		t.Fatalf("expected no document to be created, got %s", body)
	}

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/book/b1", `{"year":2000}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/movie", `[{"id":"m1","title":"Alien (1979)"}]`)
}

func TestQueryMultiFieldSortsAreOrderedAndDeterministic(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/flm/create", `{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/flm", `[{"id":"f1","title":"C","year":1983},{"id":"f2","title":"B","year":1982},{"id":"f3","title":"A","year":1979},{"id":"f4","title":"A","year":1983}]`)

	for i := 0; i < 20; i++ {
		ids := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", fmt.Sprintf(`{"entity":"flm","sorts":{"year":"desc","title":"asc"},"limit":%d}`, 10+i)).ids(t)
		if !reflect.DeepEqual(ids, []string{"f4", "f1", "f2", "f3"}) {
			t.Fatalf("year desc then title asc: expected [f4 f1 f2 f3], got %v", ids)
		}

		ids = s.mustDo(fasthttp.StatusOK, "POST", "/api/query", fmt.Sprintf(`{"entity":"flm","sorts":{"title":"asc","year":"desc"},"limit":%d}`, 10+i)).ids(t)
		if !reflect.DeepEqual(ids, []string{"f4", "f3", "f2", "f1"}) {
			t.Fatalf("title asc then year desc: expected [f4 f3 f2 f1], got %v", ids)
		}
	}

	ids := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"flm","sorts":{"year":"desc","title":"asc"},"offset":1,"limit":2}`).ids(t)
	if !reflect.DeepEqual(ids, []string{"f1", "f2"}) {
		t.Fatalf("expected pagination after sorting, got %v", ids)
	}

	ids = s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"flm","sorts":{"year":"asc"}}`).ids(t)
	if ids[0] != "f3" || ids[3] != "f1" && ids[3] != "f4" {
		t.Fatalf("expected a single sort to keep working, got %v", ids)
	}
}

func TestPreReadVirtualFieldFiltersApplyBeforePagination(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/num/create", `{"fields":{"n":{"type":"number","required":true}}}`)
	for i := 1; i <= 6; i++ {
		s.mustDo(fasthttp.StatusOK, "POST", "/api/num", fmt.Sprintf(`{"id":"v%d","n":%d}`, i, i))
	}
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/num", `{"name":"even","event":"pre_read","language":"javascript","priority":10,"enabled":true,"script":"function preRead(ctx) { ctx.entity.even = (ctx.entity.n % 2 === 0) }"}`)

	cases := map[string][]string{
		"/api/num?filter%5Beven%5D%5Beq%5D=true&limit=3&sort%5Bn%5D=asc":                 {"v2", "v4", "v6"},
		"/api/num?filter%5Beven%5D%5Beq%5D=true&sort%5Bn%5D=asc":                         {"v2", "v4", "v6"},
		"/api/num?filter%5Beven%5D%5Beq%5D=true&limit=1&offset=1&sort%5Bn%5D=asc":        {"v4"},
		"/api/num?filter%5Beven%5D%5Beq%5D=true&filter%5Bn%5D%5Bgt%5D=2&sort%5Bn%5D=asc": {"v4", "v6"},
		"/api/num?limit=2&offset=1&sort%5Bn%5D=asc":                                      {"v2", "v3"},
	}
	for path, want := range cases {
		if ids := s.mustDo(fasthttp.StatusOK, "GET", path, "").ids(t); !reflect.DeepEqual(ids, want) {
			t.Fatalf("GET %s: expected %v, got %v", path, want, ids)
		}
	}

	page := s.mustDo(fasthttp.StatusOK, "GET", "/api/num?limit=2&offset=1&sort%5Bn%5D=asc", "").list(t)
	if page[0]["even"] != true || page[1]["even"] != false {
		t.Fatalf("expected the pre_read hook to run on a page without filter, got %v", page)
	}

	queries := map[string][]string{
		`{"entity":"num","filters":{"even":{"eq":"true"}},"sorts":{"n":"asc"},"limit":3}`:                  {"v2", "v4", "v6"},
		`{"entity":"num","filters":{"even":{"eq":true}},"sorts":{"n":"asc"},"limit":2}`:                    {"v2", "v4"},
		`{"entity":"num","filters":{"even":{"eq":"true"}},"sorts":{"n":"asc"},"offset":2}`:                 {"v6"},
		`{"entity":"num","filters":{"and":[{"even":{"eq":"true"}},{"n":{"gte":4}}]},"sorts":{"n":"desc"}}`: {"v6", "v4"},
		`{"entity":"num","filters":{"n":{"lte":2}},"sorts":{"n":"asc"}}`:                                   {"v1", "v2"},
	}
	for body, want := range queries {
		if ids := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", body).ids(t); !reflect.DeepEqual(ids, want) {
			t.Fatalf("POST /api/query %s: expected %v, got %v", body, want, ids)
		}
	}

	if body := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"num","filters":{"even":{"eq":"true"}},"countOnly":true}`).body; string(body) != `{"count":3}` {
		t.Fatalf("expected the count to include virtual field filters, got %s", body)
	}
}

func TestQueryFilterValuesAndErrors(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/film", `[{"id":"f1","title":"C","year":1983,"color":true},{"id":"f2","title":"B","year":1982.5,"color":false}]`)

	cases := map[string][]string{
		`{"entity":"film","filters":{"year":{"eq":1983}}}`:   {"f1"},
		`{"entity":"film","filters":{"year":{"eq":1982.5}}}`: {"f2"},
		`{"entity":"film","filters":{"year":{"eq":"1983"}}}`: {"f1"},
		`{"entity":"film","filters":{"color":{"eq":false}}}`: {"f2"},
	}
	for body, want := range cases {
		if ids := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", body).ids(t); !reflect.DeepEqual(ids, want) {
			t.Fatalf("POST /api/query %s: expected %v, got %v", body, want, ids)
		}
	}

	for _, body := range []string{`{"entity":"film","filters":{"year":{"eq":null}}}`, `{"entity":"film","filters":{"year":{"eq":[1]}}}`, `{"entity":"film","filters":{"or":{}}}`, `{bad`} {
		resp := s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/query", body)
		if msg, _ := resp.object(t)["error"].(string); msg == "" {
			t.Fatalf("POST /api/query %s: expected an error message, got %s", body, resp.body)
		}
	}
}

func TestStringFilterOperators(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/tg", `[{"id":"t1","tags":["Smith, John","x"],"label":"banana","day":"2024-05-01"},{"id":"t2","tags":["a"],"label":"apple","day":"2024-01-01"},{"id":"t3","tags":["b"],"label":"cherry","day":"2023-01-01"}]`)

	cases := map[string][]string{
		"filter%5Btags%5D%5Bcontains%5D=Smith,%20John":                           {"t1"},
		"filter%5Btags%5D%5Bcontains%5D=a,zzz":                                   nil,
		"filter%5Btags%5D%5Bcontains%5D=a":                                       {"t2"},
		"filter%5Btags%5D%5Bnot_contains%5D=a":                                   {"t1", "t3"},
		"filter%5Btags%5D%5Bany%5D=a,b":                                          {"t2", "t3"},
		"filter%5Blabel%5D%5Bcontains%5D=an":                                     {"t1"},
		"filter%5Blabel%5D%5Bcontains%5D=AN":                                     {"t1"},
		"filter%5Blabel%5D%5Bnot_contains%5D=an":                                 {"t2", "t3"},
		"filter%5Blabel%5D%5Blt%5D=b":                                            {"t2"},
		"filter%5Blabel%5D%5Bgte%5D=banana":                                      {"t1", "t3"},
		"filter%5Blabel%5D%5Bgt%5D=zzz":                                          nil,
		"filter%5Blabel%5D%5Bany%5D=apple":                                       nil,
		"filter%5Bday%5D%5Blt%5D=2024-02-01":                                     {"t2", "t3"},
		"filter%5Bday%5D%5Bcontains%5D=-05-":                                     {"t1"},
		"filter%5Bday%5D%5Bgte%5D=2024-01-01&filter%5Bday%5D%5Blt%5D=2024-05-01": {"t2"},
	}
	for query, want := range cases {
		ids := s.mustDo(fasthttp.StatusOK, "GET", "/api/tg?"+query, "").ids(t)
		if !reflect.DeepEqual(sortedCopy(ids), want) {
			t.Fatalf("GET /api/tg?%s: expected %v, got %v", query, want, ids)
		}
	}

	if ids := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"tg","filters":{"label":{"lt":"b"}}}`).ids(t); !reflect.DeepEqual(ids, []string{"t2"}) {
		t.Fatalf("expected the Query API to compare strings too, got %v", ids)
	}
}

func TestCORSAllowList(t *testing.T) {
	s := startAdminSession(t, func(cfg *configuration.Config) {
		cfg.Server.HTTP.CORS.AllowedOrigins = []string{"https://app.example"}
	})

	resp := s.mustDo(fasthttp.StatusOK, "GET", "/api/book", "", "Origin", "https://evil.example")
	if got := resp.header("Access-Control-Allow-Origin"); got != "" {
		t.Fatalf("expected no CORS header for an unknown origin, got %q", got)
	}

	resp = s.mustDo(fasthttp.StatusNoContent, "OPTIONS", "/api/book", "", "Origin", "https://evil.example")
	if got := resp.header("Access-Control-Allow-Credentials"); got != "" {
		t.Fatalf("expected no credentials header on a preflight from an unknown origin, got %q", got)
	}

	resp = s.mustDo(fasthttp.StatusOK, "GET", "/api/book", "", "Origin", "https://app.example")
	if resp.header("Access-Control-Allow-Origin") != "https://app.example" || resp.header("Access-Control-Allow-Credentials") != "true" {
		t.Fatalf("expected CORS headers for an allowed origin, got %v", resp.headers)
	}
}

func TestCORSIsDisabledByDefault(t *testing.T) {
	s := startAdminSession(t, nil)

	resp := s.mustDo(fasthttp.StatusOK, "GET", "/api/book", "", "Origin", "https://evil.example")
	if got := resp.header("Access-Control-Allow-Origin"); got != "" {
		t.Fatalf("expected no CORS header without an allow-list, got %q", got)
	}
}

func TestSessionCookieSecureFlagFollowsTheRequest(t *testing.T) {
	s := startAPIServer(t, nil)
	body := `{"username":"admin","password":"admin"}`

	plain := s.mustDo(fasthttp.StatusOK, "POST", "/api/security/login", body)
	if strings.Contains(strings.ToLower(plain.header("Set-Cookie")), "secure") {
		t.Fatalf("expected no Secure flag over plain HTTP, got %q", plain.header("Set-Cookie"))
	}
	if !strings.Contains(strings.ToLower(plain.header("Set-Cookie")), "httponly") {
		t.Fatalf("expected the cookie to stay HttpOnly, got %q", plain.header("Set-Cookie"))
	}

	proxied := s.mustDo(fasthttp.StatusOK, "POST", "/api/security/login", body, "X-Forwarded-Proto", "https")
	if !strings.Contains(strings.ToLower(proxied.header("Set-Cookie")), "secure") {
		t.Fatalf("expected the Secure flag behind an HTTPS proxy, got %q", proxied.header("Set-Cookie"))
	}
}

func TestNestedWritePurgesSubEntityCache(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/author", `{"id":"a1","name":"Old"}`)
	s.mustDo(fasthttp.StatusOK, "GET", "/api/author", "")
	s.mustDo(fasthttp.StatusOK, "GET", "/api/author/a1", "")
	s.mustDo(fasthttp.StatusOK, "POST", "/api/novel", `{"id":"nv1","title":"T","author":{"@entity":"author","id":"a1","name":"New"}}`)

	list := s.mustDo(fasthttp.StatusOK, "GET", "/api/author", "")
	if list.header("X-Elysian-Cache") != "MISS" || list.list(t)[0]["name"] != "New" {
		t.Fatalf("expected the author list to be refreshed after a nested write, got %s (%s)", list.body, list.header("X-Elysian-Cache"))
	}

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/author/a1", "").object(t)["name"]; got != "New" {
		t.Fatalf("expected the author to be refreshed after a nested write, got %v", got)
	}

	s.mustDo(fasthttp.StatusOK, "GET", "/api/author", "")
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/novel/nv1", `{"author":{"@entity":"author","id":"a1","name":"Newer"}}`)
	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/author", "").list(t)[0]["name"]; got != "Newer" {
		t.Fatalf("expected the author list to be refreshed after a nested update, got %v", got)
	}

	s.mustDo(fasthttp.StatusOK, "GET", "/api/author", "")
	tx := beginTransaction(t, s)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/entity/novel", `{"id":"nv2","title":"U","author":{"@entity":"author","id":"a1","name":"Newest"}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/commit", "")
	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/author", "").list(t)[0]["name"]; got != "Newest" {
		t.Fatalf("expected the author list to be refreshed after a nested transactional write, got %v", got)
	}
}

func TestArrayPostIsAtomic(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/arr", `{"id":"p0","a":"x"}`)
	resp := s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/arr", `[{"id":"p1","a":"y"},{"id":"p2","a":5}]`)
	if string(resp.body) != `[[],[{"Field":"a","Message":"expected type string but got number"}]]` {
		t.Fatalf("expected per-item validation errors, got %s", resp.body)
	}

	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/arr/p1", "")
	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/arr/p2", "")

	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/arr2", `[{"id":"q1","b":"y"},{"id":"q2","b":5}]`)
	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/arr2/q1", "")

	s.mustDo(fasthttp.StatusOK, "POST", "/api/arr", `[{"id":"p3","a":"y"},{"id":"p4","a":"z","c":1}]`)
	s.mustDo(fasthttp.StatusOK, "GET", "/api/arr/p4", "")
}

func TestCoreEntityACLReturnsClearError(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"ops","password":"ops-pass-1","role":"admin"}`)
	for _, entity := range []string{"_elysiandb_core_user", "_elysiandb_core_acl", "_elysiandb_core_hook"} {
		resp := s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/acl/ops/"+entity, `{"permissions":{"read":true}}`)
		if !strings.Contains(string(resp.body), "core entities have no ACL") {
			t.Fatalf("expected a clear error for %s, got %s", entity, resp.body)
		}

		s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/acl/ops/"+entity+"/default", "")
		s.mustDo(fasthttp.StatusNotFound, "GET", "/api/acl/ops/"+entity, "")
	}

	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"title":"x"}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/acl/ops/book", `{"permissions":{"read":true}}`)
}

func TestDefaultAndLastAdminAreProtected(t *testing.T) {
	s := startAdminSession(t, nil)

	resp := s.mustDo(fasthttp.StatusForbidden, "DELETE", "/api/security/user/admin", "")
	if !strings.Contains(string(resp.body), "cannot be deleted") {
		t.Fatalf("expected a clear error, got %s", resp.body)
	}

	resp = s.mustDo(fasthttp.StatusConflict, "PUT", "/api/security/user/admin/role", `{"role":"user"}`)
	if !strings.Contains(string(resp.body), "at least one admin") {
		t.Fatalf("expected a clear error, got %s", resp.body)
	}

	s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/security/user/admin/role", `{"role":"superuser"}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/admin/role", `{"role":"admin"}`)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"ops","password":"ops-pass-1","role":"admin"}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/admin/role", `{"role":"user"}`)
	s.mustDo(fasthttp.StatusConflict, "PUT", "/api/security/user/ops/role", `{"role":"user"}`)
	s.mustDo(fasthttp.StatusConflict, "DELETE", "/api/security/user/ops", "")
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/admin/role", `{"role":"admin"}`)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"alice","password":"alice-pass","role":"user"}`)
	s.mustDo(fasthttp.StatusOK, "DELETE", "/api/security/user/alice", "")
	s.mustDo(fasthttp.StatusOK, "DELETE", "/api/security/user/ops", "")
}

func TestCoreEntitiesAreNotExposedByGenericRoutes(t *testing.T) {
	modes := map[string]func(cfg *configuration.Config){
		"disabled": func(cfg *configuration.Config) { cfg.Security.Authentication.Enabled = false },
		"token":    func(cfg *configuration.Config) { cfg.Security.Authentication.Mode = "token" },
		"user":     nil,
	}

	for name, configure := range modes {
		t.Run(name, func(t *testing.T) {
			s := startAPIServer(t, configure)
			switch name {
			case "token":
				s.headers["Authorization"] = "Bearer test-token"
			case "user":
				s.login("admin", "admin")
			}

			requests := [][]string{
				{"GET", "/api/_elysiandb_core_user", ""},
				{"GET", "/api/_elysiandb_core_user/admin", ""},
				{"DELETE", "/api/_elysiandb_core_user/admin", ""},
				{"PUT", "/api/_elysiandb_core_user/admin", `{"role":"user"}`},
				{"POST", "/api/_elysiandb_core_user", `{"id":"x","username":"x","password":"x","role":"admin"}`},
				{"GET", "/api/_elysiandb_core_acl", ""},
				{"GET", "/api/_elysiandb_core_hook/count", ""},
				{"GET", "/api/_elysiandb_core_schema/schema", ""},
				{"POST", "/api/query", `{"entity":"_elysiandb_core_user"}`},
			}

			for _, r := range requests {
				resp := s.mustDo(fasthttp.StatusForbidden, r[0], r[1], r[2])
				if strings.Contains(string(resp.body), "password") {
					t.Fatalf("%s %s leaked user data: %s", r[0], r[1], resp.body)
				}
			}

			tx := beginTransaction(t, s)
			s.mustDo(fasthttp.StatusForbidden, "POST", "/api/tx/"+tx+"/entity/_elysiandb_core_user", `{"id":"x"}`)
			s.mustDo(fasthttp.StatusForbidden, "DELETE", "/api/tx/"+tx+"/entity/_elysiandb_core_user/admin", "")

			s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"title":"still works"}`)
		})
	}
}

func TestConfigRedactsSecrets(t *testing.T) {
	s := startAPIServer(t, func(cfg *configuration.Config) {
		cfg.Security.Authentication.Mode = "token"
		cfg.Engine.URI = "mongodb://elysian:s3cret@localhost:27017/elysiandb"
	})
	s.headers["Authorization"] = "Bearer test-token"

	resp := s.mustDo(fasthttp.StatusOK, "GET", "/config", "")
	if strings.Contains(string(resp.body), "test-token") || strings.Contains(string(resp.body), "s3cret") {
		t.Fatalf("expected secrets to be redacted, got %s", resp.body)
	}

	authentication := resp.object(t)["Security"].(map[string]any)["Authentication"].(map[string]any)
	if authentication["Token"] != "***" || authentication["Mode"] != "token" || authentication["Enabled"] != true {
		t.Fatalf("unexpected authentication block %v", authentication)
	}

	if uri := resp.object(t)["Engine"].(map[string]any)["URI"]; uri != "mongodb://elysian:xxxxx@localhost:27017/elysiandb" {
		t.Fatalf("expected the engine URI password to be redacted, got %v", uri)
	}

	if globals.GetConfig().Security.Authentication.Token != "test-token" {
		t.Fatalf("redaction must not change the running configuration")
	}
}

func sortedCopy(in []string) []string {
	if len(in) == 0 {
		return nil
	}

	out := append([]string(nil), in...)
	sort.Strings(out)

	return out
}

func withoutStrictSchema(cfg *configuration.Config) {
	cfg.Api.Schema.Strict = false
}

func TestUnvalidatedWritesKeepTheInferredSchemaInStep(t *testing.T) {
	s := startAdminSession(t, withoutStrictSchema)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/p", `{"id":"a","n":"x"}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/p/a", `{"n":5}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/p", `{"id":"b","n":6}`)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/items", `{"id":"a","status":"active"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/items/migrate", `[{"set":[{"status":1}]}]`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/items", `{"id":"b","status":2}`)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/author", `{"id":"x","age":30}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"id":"b","author":{"@entity":"author","id":"z","age":"thirty"}}`)
	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/author/z", "").object(t)["age"]; got != "thirty" {
		t.Fatalf("expected the nested author to be stored, got %v", got)
	}

	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/author", `{"id":"w","age":5}`)
}

func TestImportKeepsDocumentsWithMixedTypes(t *testing.T) {
	s := startAdminSession(t, withoutStrictSchema)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/p", `{"id":"a","n":"x"}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/p/a", `{"n":5}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/p", `{"id":"b","n":6}`)

	dump := s.mustDo(fasthttp.StatusOK, "GET", "/api/export", "").object(t)
	body, _ := json.Marshal(map[string]any{"p": dump["p"]})
	s.mustDo(fasthttp.StatusOK, "POST", "/api/import", string(body))
	if ids := sortedCopy(s.mustDo(fasthttp.StatusOK, "GET", "/api/p", "").ids(t)); !reflect.DeepEqual(ids, []string{"a", "b"}) {
		t.Fatalf("expected an export/import round trip to keep every document, got %v", ids)
	}

	s.mustDo(fasthttp.StatusOK, "POST", "/api/import", `{"mixed":[{"id":"m1","v":"x"},{"id":"m2","w":1},{"id":"m3","v":2}]}`)
	if ids := sortedCopy(s.mustDo(fasthttp.StatusOK, "GET", "/api/mixed", "").ids(t)); !reflect.DeepEqual(ids, []string{"m1", "m2", "m3"}) {
		t.Fatalf("expected the import to keep documents with mixed types, got %v", ids)
	}

	s.mustDo(fasthttp.StatusOK, "POST", "/api/strictimp/create", `{"fields":{"title":{"type":"string","required":true}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/import", `{"strictimp":[{"id":"s1","title":"ok"},{"id":"s2","title":5}]}`)
	if ids := s.mustDo(fasthttp.StatusOK, "GET", "/api/strictimp", "").ids(t); !reflect.DeepEqual(ids, []string{"s1"}) {
		t.Fatalf("expected manual schemas to keep validating imports, got %v", ids)
	}
}

func TestDestroyResetsTheInferredSchemaOnly(t *testing.T) {
	s := startAdminSession(t, withoutStrictSchema)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/sz", `{"id":"1","size":"big"}`)
	s.mustDo(fasthttp.StatusNoContent, "DELETE", "/api/sz", "")
	s.mustDo(fasthttp.StatusOK, "POST", "/api/sz", `{"id":"2","color":"red"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/sz", `{"id":"3","size":4}`)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/man/create", `{"fields":{"title":{"type":"string","required":true}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/man", `{"id":"1","title":"x"}`)
	s.mustDo(fasthttp.StatusNoContent, "DELETE", "/api/man", "")
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/man", `{"id":"2","title":5}`)
}

func TestNestedCoreEntityReferencesAreRejected(t *testing.T) {
	for name, configure := range map[string]func(cfg *configuration.Config){
		"disabled": func(cfg *configuration.Config) { cfg.Security.Authentication.Enabled = false },
		"user":     nil,
	} {
		t.Run(name, func(t *testing.T) {
			s := startAPIServer(t, configure)
			if configure == nil {
				s.login("admin", "admin")
			}

			hookRef := `{"@entity":"_elysiandb_core_hook","name":"h","entity":"posts","event":"pre_read","language":"javascript","enabled":true,"script":"function preRead(ctx){ctx.entity.pwn=1}"}`
			userRef := `{"@entity":"_elysiandb_core_user","id":"admin"}`

			s.mustDo(fasthttp.StatusOK, "POST", "/api/posts", `{"id":"p1","title":"t"}`)
			s.mustDo(fasthttp.StatusForbidden, "POST", "/api/posts", `{"id":"p2","x":`+hookRef+`}`)
			s.mustDo(fasthttp.StatusForbidden, "POST", "/api/posts", `[{"id":"p3","items":[`+userRef+`]}]`)
			s.mustDo(fasthttp.StatusForbidden, "PUT", "/api/posts/p1", `{"owner":`+userRef+`}`)
			s.mustDo(fasthttp.StatusForbidden, "PUT", "/api/posts", `[{"id":"p1","meta":{"deep":`+hookRef+`}}]`)

			tx := beginTransaction(t, s)
			s.mustDo(fasthttp.StatusForbidden, "POST", "/api/tx/"+tx+"/entity/posts", `{"id":"p4","x":`+hookRef+`}`)
			s.mustDo(fasthttp.StatusForbidden, "PUT", "/api/tx/"+tx+"/entity/posts/p1", `{"owner":`+userRef+`}`)
			s.mustDo(fasthttp.StatusOK, "POST", "/api/tx/"+tx+"/commit", "")

			if hooks := s.mustDo(fasthttp.StatusOK, "GET", "/api/hook/posts", "").list(t); len(hooks) != 0 {
				t.Fatalf("expected no hook to be created through a nested write, got %v", hooks)
			}

			post := s.mustDo(fasthttp.StatusOK, "GET", "/api/posts/p1", "").object(t)
			if _, ok := post["owner"]; ok || post["pwn"] != nil {
				t.Fatalf("expected the post to be left untouched, got %v", post)
			}

			s.mustDo(fasthttp.StatusNotFound, "GET", "/api/posts/p2", "")
		})
	}
}

func TestIncludesDoNotResolveCoreEntities(t *testing.T) {
	s := startAPIServer(t, func(cfg *configuration.Config) { cfg.Security.Authentication.Enabled = false })

	api_storage.WriteEntity("leak", map[string]any{"id": "l1", "u": map[string]any{"@entity": "_elysiandb_core_user", "id": "admin"}, "w": map[string]any{"@entity": "writer", "id": "w1"}})
	api_storage.WriteEntity("writer", map[string]any{"id": "w1", "name": "W"})

	for _, path := range []string{"/api/leak/l1?includes=u,w", "/api/leak?includes=all", "/api/leak?includes=u"} {
		resp := s.mustDo(fasthttp.StatusOK, "GET", path, "")
		if strings.Contains(string(resp.body), "password") || strings.Contains(string(resp.body), `"role"`) {
			t.Fatalf("GET %s resolved a core entity: %s", path, resp.body)
		}
	}

	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/leak/l1?includes=w", "").object(t)["w"].(map[string]any)["name"]; got != "W" {
		t.Fatalf("expected regular includes to keep working, got %v", got)
	}
}

func TestPreReadHooksDoNotScanTheWholeEntityForAPage(t *testing.T) {
	s := startAdminSession(t, withoutStrictSchema)

	posts := make([]map[string]any, 0, 300)
	authors := make([]map[string]any, 0, 300)
	for i := 0; i < 300; i++ {
		posts = append(posts, map[string]any{"id": fmt.Sprintf("p%03d", i), "authorId": fmt.Sprintf("a%03d", i), "rank": i})
		authors = append(authors, map[string]any{"id": fmt.Sprintf("a%03d", i), "name": fmt.Sprintf("Author %d", i)})
	}
	postsBody, _ := json.Marshal(posts)
	authorsBody, _ := json.Marshal(authors)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/post", string(postsBody))
	s.mustDo(fasthttp.StatusOK, "POST", "/api/author", string(authorsBody))
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/post", `{"name":"join","event":"pre_read","language":"javascript","enabled":true,"bypass_acl":true,"script":"function preRead(ctx){ var r = ctx.query(\"author\", {\"id\": {\"eq\": ctx.entity.authorId}}); ctx.entity.authorName = r.length ? r[0].name : null }"}`)

	elapsed := func(method, path, body string) time.Duration {
		start := time.Now()
		s.mustDo(fasthttp.StatusOK, method, path, body)
		return time.Since(start)
	}

	full := elapsed("POST", "/api/query", `{"entity":"post","filters":{"authorName":{"eq":"Author*"}}}`)

	page := elapsed("POST", "/api/query", `{"entity":"post","limit":5}`)
	one := elapsed("POST", "/api/query", `{"entity":"post","filters":{"id":{"eq":"p007"}}}`)
	firstMatches := elapsed("POST", "/api/query", `{"entity":"post","filters":{"authorName":{"eq":"Author*"}},"limit":5}`)
	rest := elapsed("GET", "/api/post?filter%5BauthorName%5D%5Beq%5D=Author*&limit=5", "")

	for name, d := range map[string]time.Duration{"page": page, "stored filter": one, "virtual filter with limit": firstMatches, "REST virtual filter with limit": rest} {
		if d*10 > full {
			t.Fatalf("%s took %v, expected far less than a full scan (%v)", name, d, full)
		}
	}

	resp := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"post","filters":{"id":{"eq":"p007"}}}`).list(t)
	if len(resp) != 1 || resp[0]["authorName"] != "Author 7" {
		t.Fatalf("expected the hook to run on the matching document, got %v", resp)
	}

	if ids := s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"post","filters":{"authorName":{"eq":"Author 1*"}},"sorts":{"rank":"asc"},"offset":2,"limit":2}`).ids(t); !reflect.DeepEqual(ids, []string{"p011", "p012"}) {
		t.Fatalf("expected pagination over the virtual matches, got %v", ids)
	}
}
