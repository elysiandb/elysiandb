package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"math/rand"
	"os"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/storage"
	api_controller "github.com/taymour/elysiandb/internal/transport/http/api"
	"github.com/taymour/elysiandb/test/golden/goldenio"
	"github.com/valyala/fasthttp"
)

func main() {
	if os.Getenv("TZ") != "UTC" {
		fmt.Fprintln(os.Stderr, "run with TZ=UTC")
		os.Exit(2)
	}
	goldenio.Main("api", run)
}

var tmpRoot string

func run(out string) error {
	dir, err := os.MkdirTemp("", "elysiandb-golden-api-")
	if err != nil {
		return err
	}
	tmpRoot = dir
	defer os.RemoveAll(dir)
	steps := []func(string) error{genRest, genQueryErrors, genMatcher, genPlanner, genSort, genInclude, genWrite, genSchema, genScalars, genCacheKeys}
	for _, s := range steps {
		if err := s(out); err != nil {
			return err
		}
	}
	return nil
}

func bsl(s string) string {
	return strings.ReplaceAll(s, "~", "\\")
}

func hs(s string) string { return hex.EncodeToString([]byte(s)) }

func textOrHex(row map[string]any, field string, b []byte) {
	if utf8.Valid(b) {
		row[field] = string(b)
	} else {
		row[field+"_hex"] = hex.EncodeToString(b)
	}
}

var storeCounter int

func openStore(schemaOn, strict, cacheOn bool) {
	storeCounter++
	cfg := &configuration.Config{}
	cfg.Store.Folder = fmt.Sprintf("%s/store-%03d", tmpRoot, storeCounter)
	cfg.Store.Shards = 4
	cfg.Api.Cache.Enabled = cacheOn
	cfg.Api.Schema.Enabled = schemaOn
	cfg.Api.Schema.Strict = strict
	cfg.Api.Hooks.Enabled = false
	cfg.Security.Authentication.Enabled = false
	cfg.Log.Level = "error"
	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()
	cache.InitCache(30 * time.Second)
	api_storage.DeleteAll()
}

func closeStore() {
	_ = storage.Close()
}

type uuidMasker struct {
	seen map[string]string
}

var uuidPattern = regexp.MustCompile(`[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}`)

func (m *uuidMasker) mask(b []byte) []byte {
	return uuidPattern.ReplaceAllFunc(b, func(u []byte) []byte {
		name, ok := m.seen[string(u)]
		if !ok {
			name = fmt.Sprintf("<uuid#%d>", len(m.seen)+1)
			m.seen[string(u)] = name
		}
		return []byte(name)
	})
}

type request struct {
	name    string
	handler string
	method  string
	uri     string
	body    string
	entity  string
	id      string
}

var handlers = map[string]fasthttp.RequestHandler{
	"list":        api_controller.RejectCoreEntity(api_controller.ListController),
	"query":       api_controller.QueryController,
	"count":       api_controller.RejectCoreEntity(api_controller.CountController),
	"exists":      api_controller.RejectCoreEntity(api_controller.ExistsController),
	"get":         api_controller.RejectCoreEntity(api_controller.GetByIdController),
	"create":      api_controller.RejectCoreEntity(api_controller.CreateController),
	"update":      api_controller.RejectCoreEntity(api_controller.UpdateByIdController),
	"update_list": api_controller.RejectCoreEntity(api_controller.UpdateListController),
	"delete":      api_controller.RejectCoreEntity(api_controller.DeleteByIdController),
	"destroy":     api_controller.RejectCoreEntity(api_controller.DestroyController),
	"migrate":     api_controller.RejectCoreEntity(api_controller.MigrateController),
	"put_schema":  api_controller.RejectCoreEntity(api_controller.PutSchemaController),
	"get_schema":  api_controller.RejectCoreEntity(api_controller.GetSchemaController),
	"create_type": api_controller.RejectCoreEntity(api_controller.CreateTypeController),
	"types":       api_controller.GetEntityTypesController,
	"types_names": api_controller.GetEntityTypesNamesController,
}

var writeHandlers = map[string]bool{"create": true, "update": true, "update_list": true, "delete": true, "destroy": true, "migrate": true, "put_schema": true, "create_type": true}

func goDecodeErrors(body string) []error {
	var errs []error
	var p api_controller.QueryPayload
	if err := json.Unmarshal([]byte(body), &p); err != nil {
		errs = append(errs, err)
	}
	var anyV any
	if err := json.Unmarshal([]byte(body), &anyV); err != nil {
		errs = append(errs, err)
	}
	var m map[string]any
	if err := json.Unmarshal([]byte(body), &m); err != nil {
		errs = append(errs, err)
	}
	var list []map[string]any
	if err := json.Unmarshal([]byte(body), &list); err != nil {
		errs = append(errs, err)
	}
	return errs
}

func goErrorInfo(err error) map[string]any {
	var se *json.SyntaxError
	var te *json.UnmarshalTypeError
	switch {
	case errors.As(err, &se):
		return map[string]any{"message_kind": "go", "go_offset": se.Offset}
	case errors.As(err, &te):
		return map[string]any{"message_kind": "go", "go_field": te.Field, "go_value": te.Value}
	}
	return map[string]any{"message_kind": "go"}
}

func jsonString(s string) string {
	b, _ := json.Marshal(s)
	return string(b[1 : len(b)-1])
}

func call(r request) (*fasthttp.RequestCtx, bool) {
	req := fasthttp.AcquireRequest()
	req.SetRequestURI(r.uri)
	req.Header.SetMethod(r.method)
	if r.body != "" {
		req.SetBodyString(r.body)
	}
	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	if r.entity != "" {
		ctx.SetUserValue("entity", r.entity)
	}
	if r.id != "" {
		ctx.SetUserValue("id", r.id)
	}
	panicked := false
	func() {
		defer func() {
			if rec := recover(); rec != nil {
				panicked = true
				ctx.Response.Reset()
				ctx.Response.Header.Set("Content-Type", "application/json")
				ctx.SetStatusCode(fasthttp.StatusInternalServerError)
			}
		}()
		handlers[r.handler](ctx)
	}()
	return ctx, panicked
}

func storedEntity(entity string, m *uuidMasker) []any {
	var out []any
	docs := api_storage.ListEntities(entity, 0, 0, "", true, nil, "", "")
	for _, d := range docs {
		id, _ := d["id"].(string)
		b, ok := api_storage.ReadEntityBytesById(entity, id)
		if !ok {
			continue
		}
		row := map[string]any{"id": string(m.mask([]byte(id)))}
		textOrHex(row, "bytes", m.mask(b))
		out = append(out, row)
	}
	return out
}

func runRequests(set string, reqs []request, lines *goldenio.Lines, m *uuidMasker) {
	for _, r := range reqs {
		ctx, panicked := call(r)
		row := map[string]any{"set": set, "name": r.name, "handler": r.handler, "method": r.method, "uri": r.uri, "entity": r.entity, "id": r.id}
		textOrHex(row, "request_body", []byte(r.body))
		row["status"] = ctx.Response.StatusCode()
		row["content_type"] = string(ctx.Response.Header.ContentType())
		row["x_elysian_cache"] = string(ctx.Response.Header.Peek("X-Elysian-Cache"))
		body := ctx.Response.Body()
		row["panic"] = panicked
		masked := false
		if ctx.Response.StatusCode() >= 400 && r.body != "" {
			for _, err := range goDecodeErrors(r.body) {
				if strings.Contains(string(body), jsonString(err.Error())) {
					for k, v := range goErrorInfo(err) {
						row[k] = v
					}
					masked = true
					break
				}
			}
		}
		if !masked {
			if errs, ok := validationErrors(body); ok {
				row["validation_errors_sorted"] = errs
			} else {
				textOrHex(row, "body", m.mask(body))
			}
		}
		if writeHandlers[r.handler] && r.entity != "" {
			row["stored"] = storedEntity(r.entity, m)
			if b, ok := api_storage.ReadEntityBytesById("_elysiandb_core_schema", r.entity); ok {
				textOrHex(row, "schema_document", m.mask(b))
			}
		}
		lines.Add(row)
	}
}

func list(name, entity, qs string) request {
	uri := "/api/" + entity
	if qs != "" {
		uri += "?" + qs
	}
	return request{name: name, handler: "list", method: "GET", uri: uri, entity: entity}
}

func q(name, body string) request {
	return request{name: name, handler: "query", method: "POST", uri: "/api/query", body: body}
}

func seedBooks() {
	api_storage.WriteEntity("person", map[string]any{"id": "p1", "name": "Frank"})
	api_storage.WriteEntity("book", map[string]any{"id": "b1", "title": "Dune", "year": 1965.0, "tags": []any{"sf", "classic"}, "author": map[string]any{"@entity": "person", "id": "p1"}})
	api_storage.WriteEntity("book", map[string]any{"id": "b2", "title": "dune messiah", "year": "1969", "tags": []any{}, "author": map[string]any{"@entity": "person", "id": "p9"}})
	api_storage.WriteEntity("book", map[string]any{"id": "b3", "title": "Emma", "flag": false})
}

func bookReads() []request {
	reqs := []request{
		list("title-eq", "book", "filter[title]=dune"),
		list("title-glob", "book", "filter[title]=dune*"),
		list("title-contains", "book", "filter[title][contains]=UNE"),
		list("year-gt", "book", "filter[year][gt]=1960"),
		list("year-gt-lt", "book", "filter[year][gt]=1960&filter[year][lt]=2"),
		list("year-glob", "book", "filter[year]=196*"),
		list("flag-text", "book", "filter[flag]=nope"),
		list("flag-neq", "book", "filter[flag][neq]=true"),
		list("tags-eq", "book", "filter[tags][eq]=classic,sf"),
		list("tags-none", "book", "filter[tags][none]=x"),
		list("tags-any", "book", "filter[tags][any]=sf,x"),
		list("tags-all", "book", "filter[tags][all]=sf,classic"),
		list("tags-contains", "book", "filter[tags][contains]=cla"),
		list("tags-not-contains", "book", "filter[tags][not_contains]=sf"),
		list("author-name", "book", "filter[author.name]=frank"),
		list("includes-author", "book", "includes=author"),
		list("includes-tags", "book", "includes=tags"),
		list("includes-all", "book", "includes=all"),
		list("sort-desc", "book", "sort[year]=desc"),
		list("sort-asc", "book", "sort[year]=asc"),
		list("sort-title-desc", "book", "sort[title]=DESC"),
		list("sort-bad", "book", "sort[title]=up&sort[year]=desc"),
		list("count-page", "book", "countOnly=true&limit=2&offset=1"),
		list("count-all", "book", "countOnly=true"),
		list("count-yes", "book", "countOnly=yes"),
		list("count-sort", "book", "countOnly=true&sort[year]=asc"),
		list("limit-negative", "book", "limit=-1"),
		list("limit-1", "book", "limit=1"),
		list("limit-plus", "book", "limit=+1"),
		list("offset-1", "book", "offset=1"),
		list("offset-beyond", "book", "offset=10"),
		list("fields-author-name", "book", "fields=author.name&includes=author"),
		list("fields-title", "book", "fields=title,year"),
		list("search-dune", "book", "search=dune"),
		list("search-star", "book", "search=*mess*"),
		list("search-upper", "book", "search=EMMA"),
		list("filter-unknown-op", "book", "filter[title][b][c]=x"),
		list("filter-trim", "book", "filter[title]=%20Dune%20"),
		list("filter-repeat", "book", "filter[title]=x&filter[title]=Dune"),
		list("filter-missing-field", "book", "filter[x]=1"),
		list("filter-date-like", "book", "filter[year][gte]=1965&filter[year][lte]=1965"),
		list("empty-entity", "nothing", ""),
		list("core-entity", "_elysiandb_core_user", ""),
		q("or", `{"entity":"book","filters":{"or":[{"year":{"eq":1965}},{"title":{"eq":"emma"}}]}}`),
		q("and-empty", `{"entity":"book","filters":{"and":[]}}`),
		q("or-empty", `{"entity":"book","filters":{"or":[]}}`),
		q("unknown-field", `{"entity":"book","filters":{"x":{}}}`),
		q("limit-fraction", `{"entity":"book","limit":1.5}`),
		q("invalid-field-filter", `{"entity":"book","filters":{"a<b":5}}`),
		q("sorts-two", `{"entity":"book","sorts":{"year":"DESC","title":"x"}}`),
		q("sorts-title", `{"entity":"book","sorts":{"title":"asc"}}`),
		q("count-limit", `{"entity":"book","countOnly":true,"limit":1}`),
		q("count-all", `{"entity":"book","countOnly":true}`),
		q("fields", `{"entity":"book","fields":"title"}`),
		q("offset-limit", `{"entity":"book","offset":1,"limit":1}`),
		q("nested-and-or", `{"entity":"book","filters":{"and":[{"or":[{"title":{"eq":"dune*"}},{"flag":{"eq":false}}]},{"year":{"neq":"1969"}}]}}`),
		q("number-format", `{"entity":"book","filters":{"year":{"eq":1.965e3}}}`),
		q("bool-filter", `{"entity":"book","filters":{"flag":{"eq":false}}}`),
		q("or-not-array", `{"entity":"book","filters":{"or":{}}}`),
		q("or-item", `{"entity":"book","filters":{"or":[1]}}`),
		q("and-not-array", `{"entity":"book","filters":{"and":"x"}}`),
		q("and-item", `{"entity":"book","filters":{"and":[[]]}}`),
		q("value-null", `{"entity":"book","filters":{"title":{"eq":null}}}`),
		q("value-array", `{"entity":"book","filters":{"title":{"eq":[1]}}}`),
		q("value-object", `{"entity":"book","filters":{"title":{"eq":{}}}}`),
		q("core-entity", `{"entity":"_elysiandb_core_user"}`),
		q("missing-entity", `{"entity":"nothing"}`),
		{name: "count", handler: "count", method: "GET", uri: "/api/book/count", entity: "book"},
		{name: "exists", handler: "exists", method: "GET", uri: "/api/book/b1/exists", entity: "book", id: "b1"},
		{name: "exists-missing", handler: "exists", method: "GET", uri: "/api/book/zz/exists", entity: "book", id: "zz"},
		{name: "get", handler: "get", method: "GET", uri: "/api/book/b1", entity: "book", id: "b1"},
		{name: "get-includes", handler: "get", method: "GET", uri: "/api/book/b1?includes=author", entity: "book", id: "b1"},
		{name: "get-fields", handler: "get", method: "GET", uri: "/api/book/b1?fields=title", entity: "book", id: "b1"},
		{name: "get-missing", handler: "get", method: "GET", uri: "/api/book/zz", entity: "book", id: "zz"},
		{name: "types", handler: "types", method: "GET", uri: "/api/entity/types"},
		{name: "types-names", handler: "types_names", method: "GET", uri: "/api/entity/types/name"},
	}
	return reqs
}

func bookWrites() []request {
	return []request{
		{name: "create-with-id", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `{"id":"b4","title":"New <&>","year":2000,"tags":["x"]}`},
		{name: "create-no-id", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `{"title":"Anonymous"}`},
		{name: "create-list", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `[{"id":"b5","title":"L1"},{"title":"L2"}]`},
		{name: "create-sub-entity", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `{"id":"b6","title":"Sub","author":{"@entity":"person","id":"p2","name":"Paul"}}`},
		{name: "create-sub-no-id", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `{"id":"b7","author":{"@entity":"person","name":"Nameless"}}`},
		{name: "create-invalid-json", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `{"title":`},
		{name: "create-scalar", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `5`},
		{name: "create-empty", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: ``},
		{name: "create-core-ref", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `{"id":"b8","owner":{"@entity":"_elysiandb_core_user","id":"admin"}}`},
		{name: "update", handler: "update", method: "PUT", uri: "/api/book/b1", entity: "book", id: "b1", body: `{"title":"Dune (2)","year":1966}`},
		{name: "update-missing", handler: "update", method: "PUT", uri: "/api/book/zz", entity: "book", id: "zz", body: `{"title":"x"}`},
		{name: "update-invalid", handler: "update", method: "PUT", uri: "/api/book/b1", entity: "book", id: "b1", body: `nope`},
		{name: "update-list", handler: "update_list", method: "PUT", uri: "/api/book", entity: "book", body: `[{"id":"b2","year":1970},{"id":"zz","year":1},{"title":"no id"}]`},
		{name: "migrate-set", handler: "migrate", method: "POST", uri: "/api/book/migrate", entity: "book", body: `[{"set":{"status":"kept","meta.level":2}}]`},
		{name: "migrate-array-payload", handler: "migrate", method: "POST", uri: "/api/book/migrate", entity: "book", body: `[{"set":[{"a":1},null,{}]}]`},
		{name: "migrate-unsupported", handler: "migrate", method: "POST", uri: "/api/book/migrate", entity: "book", body: `[{"drop":{"x":1}}]`},
		{name: "migrate-object", handler: "migrate", method: "POST", uri: "/api/book/migrate", entity: "book", body: `{"set":{"x":1}}`},
		{name: "migrate-scalar-element", handler: "migrate", method: "POST", uri: "/api/book/migrate", entity: "book", body: `[1]`},
		{name: "migrate-missing-entity", handler: "migrate", method: "POST", uri: "/api/nothing/migrate", entity: "nothing", body: `[{"set":{"x":1}}]`},
		{name: "migrate-through-scalar", handler: "migrate", method: "POST", uri: "/api/book/migrate", entity: "book", body: `[{"set":{"title.sub":"x"}}]`},
		{name: "delete", handler: "delete", method: "DELETE", uri: "/api/book/b3", entity: "book", id: "b3"},
		{name: "delete-missing", handler: "delete", method: "DELETE", uri: "/api/book/zz", entity: "book", id: "zz"},
		{name: "recreate-deleted", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `{"id":"b3","title":"Emma again"}`},
		{name: "list-after-writes", handler: "list", method: "GET", uri: "/api/book", entity: "book"},
		{name: "destroy", handler: "destroy", method: "DELETE", uri: "/api/person", entity: "person"},
		{name: "list-person-after-destroy", handler: "list", method: "GET", uri: "/api/person", entity: "person"},
		{name: "types-after", handler: "types", method: "GET", uri: "/api/entity/types"},
	}
}

func genRest(out string) error {
	var lines goldenio.Lines
	m := &uuidMasker{seen: map[string]string{}}

	openStore(false, false, false)
	seedBooks()
	runRequests("books", bookReads(), &lines, m)
	runRequests("books", bookWrites(), &lines, m)
	closeStore()

	openStore(false, false, true)
	seedBooks()
	var cached []request
	for _, r := range []request{list("cached-list", "book", "filter[title]=dune*"), q("cached-query", `{"entity":"book","sorts":{"year":"asc"}}`), q("cached-count", `{"entity":"book","countOnly":true}`), list("cached-count-list", "book", "countOnly=true")} {
		cached = append(cached, r, r)
	}
	cached = append(cached, request{name: "write-invalidates", handler: "create", method: "POST", uri: "/api/book", entity: "book", body: `{"id":"c1","title":"dune 3"}`}, list("cached-list-after-write", "book", "filter[title]=dune*"), list("cached-list-after-write-again", "book", "filter[title]=dune*"))
	runRequests("books-cache", cached, &lines, m)
	closeStore()

	openStore(true, false, false)
	api_storage.WriteEntity("person", map[string]any{"id": "p1", "name": "Ann"})
	api_storage.WriteEntity("person", map[string]any{"id": "p2", "name": "Bob"})
	api_storage.WriteEntity("team", map[string]any{"id": "t1", "name": "Core", "lead": map[string]any{"@entity": "person", "id": "p1"}, "members": []any{map[string]any{"@entity": "person", "id": "p1"}, map[string]any{"@entity": "person", "id": "p2"}}})
	teamReqs := []request{
		list("team-includes", "team", "includes=lead,members"),
		list("team-members-filter", "team", "filter[members.name]=bob"),
		list("team-search", "team", "search=bob&includes=members"),
		list("team-members-any", "team", "includes=members&filter[members][any]=p1"),
		{name: "team-schema", handler: "get_schema", method: "GET", uri: "/api/team/schema", entity: "team"},
		{name: "team-migrate", handler: "migrate", method: "POST", uri: "/api/team/migrate", entity: "team", body: `[{"set":{"touched":true}}]`},
		list("team-after-migrate", "team", ""),
		{name: "team-schema-after", handler: "get_schema", method: "GET", uri: "/api/team/schema", entity: "team"},
		{name: "team-create-after", handler: "create", method: "POST", uri: "/api/team", entity: "team", body: `{"id":"t2","members":[{"@entity":"person","id":"p2"}]}`},
		{name: "team-create-plain-array", handler: "create", method: "POST", uri: "/api/team", entity: "team", body: `{"id":"t3","members":["x"]}`},
	}
	runRequests("teams", teamReqs, &lines, m)
	closeStore()

	openStore(true, false, false)
	schemaReqs := []request{
		{name: "create-infers", handler: "create", method: "POST", uri: "/api/doc", entity: "doc", body: `{"id":"d1","title":"t","n":1,"ok":true,"tags":["a"],"meta":{"x":1},"nothing":null}`},
		{name: "create-type-mismatch", handler: "create", method: "POST", uri: "/api/doc", entity: "doc", body: `{"id":"d2","title":5,"n":"one","ok":"yes","tags":"a","meta":[]}`},
		{name: "create-new-field", handler: "create", method: "POST", uri: "/api/doc", entity: "doc", body: `{"id":"d3","extra":"x"}`},
		{name: "get-schema", handler: "get_schema", method: "GET", uri: "/api/doc/schema", entity: "doc"},
		{name: "get-schema-missing", handler: "get_schema", method: "GET", uri: "/api/none/schema", entity: "none"},
		{name: "create-type", handler: "create_type", method: "POST", uri: "/api/made/create", entity: "made", body: `{"fields":{"title":{"type":"string","required":true}}}`},
		{name: "create-type-again", handler: "create_type", method: "POST", uri: "/api/made/create", entity: "made", body: `{"fields":{}}`},
		{name: "create-type-invalid", handler: "create_type", method: "POST", uri: "/api/made2/create", entity: "made2", body: `{`},
		{name: "update-mismatch", handler: "update", method: "PUT", uri: "/api/doc/d1", entity: "doc", id: "d1", body: `{"n":"x"}`},
		{name: "types-after-schema", handler: "types", method: "GET", uri: "/api/entity/types"},
	}
	runRequests("schema", schemaReqs, &lines, m)
	closeStore()

	openStore(true, true, false)
	strictReqs := []request{
		{name: "put-schema", handler: "put_schema", method: "PUT", uri: "/api/art/schema", entity: "art", body: `{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"},"meta":{"type":"object","fields":{"k":{"type":"string","required":true}}}}}`},
		{name: "create-ok", handler: "create", method: "POST", uri: "/api/art", entity: "art", body: `{"id":"a1","title":"T","year":1,"meta":{"k":"v"}}`},
		{name: "create-missing-required", handler: "create", method: "POST", uri: "/api/art", entity: "art", body: `{"id":"a2","year":1}`},
		{name: "create-null-required", handler: "create", method: "POST", uri: "/api/art", entity: "art", body: `{"id":"a3","title":null}`},
		{name: "create-unknown-field", handler: "create", method: "POST", uri: "/api/art", entity: "art", body: `{"id":"a4","title":"T","color":"red"}`},
		{name: "create-nested-missing", handler: "create", method: "POST", uri: "/api/art", entity: "art", body: `{"id":"a5","title":"T","meta":{}}`},
		{name: "create-several-errors", handler: "create", method: "POST", uri: "/api/art", entity: "art", body: `{"id":"a6","title":1,"year":"x","color":1}`},
		{name: "update-ok", handler: "update", method: "PUT", uri: "/api/art/a1", entity: "art", id: "a1", body: `{"year":2}`},
		{name: "update-bad", handler: "update", method: "PUT", uri: "/api/art/a1", entity: "art", id: "a1", body: `{"year":"two"}`},
		{name: "get-schema", handler: "get_schema", method: "GET", uri: "/api/art/schema", entity: "art"},
		{name: "put-schema-invalid", handler: "put_schema", method: "PUT", uri: "/api/art/schema", entity: "art", body: `[]`},
	}
	runRequests("strict", strictReqs, &lines, m)
	closeStore()
	return lines.Write(out, "rest_corpus.jsonl")
}

func genQueryErrors(out string) error {
	openStore(false, false, false)
	defer closeStore()
	api_storage.WriteEntity("b", map[string]any{"id": "x", "t": "v"})
	bodies := []string{
		`{"entity":"b","limit":"5"}`, `{"entity":"b","limit":1.5}`, `{"entity":"b","limit":1e3}`, `{"entity":"b","limit":1.0}`, `{"entity":"b","limit":99999999999999999999}`,
		`{"entity":5}`, `{"entity":"b","countOnly":"true"}`, `{"entity":"b","fields":["a"]}`, `{"entity":"b","sorts":{"a":1}}`, `{"entity":"b","sorts":[]}`,
		`{"entity":"b","filters":[]}`, `{"entity":"b","filters":{"x":{"eq":1e400}}}`, `{"LIMIT":"x"}`, `[1]`, `not json`, ``, `{"entity":"b","limit":5,}`,
		`{"entity":"b"} x`, "{\"entity\":\"a\x01b\"}", `{"entity":"b","limit":"x","offset":"y"}`, "{\"entity\":\"\xff\"}", `{"entity":"b","offset":-1}`,
		`{"entity":"b","limit":-0}`, `{"entity":"b","Limit":2}`, `{"entity":"b","filters":null,"sorts":null}`, `{"entity":"b","filters":{"t":"v"}}`,
		`{"entity":"b","filters":{"t":{"eq":"v","neq":null}}}`, strings.Repeat("[", 10001), `{"entity":"b","sorts":{"t":"asc"},"sorts":{"t":"desc"}}`,
		`{"entity":"b","filters":{"t":{"eq":true}}}`, `{"entity":"b","filters":{"t":{"eq":1e21}}}`, `{"entity":"b","filters":{"t":{"eq":1e-7}}}`, `{"entity":"b","filters":{"t":{"eq":-0}}}`,
		`{"entity":"b","filters":{"t":{"eq":"~u0076"}}}`, `{"entity":"b","filters":{"t":{"eq":"~ud800"}}}`, `{"entity":"b","countOnly":1}`, `{"entity":"b","fields":null}`,
		` {"entity":"b"} `, "{\"entity\":\"b\"}\n", `{"entity":"b","limit":5}garbage`, `{"entity":"b",}`, `{"entity":}`, `{entity:"b"}`, `{"entity":"b"`, `"string"`, `null`, `true`, `12`,
	}
	for _, kind := range []string{"string", "number", "boolean", "array", "object", "null"} {
		val := map[string]string{"string": `"s"`, "number": `1`, "boolean": `true`, "array": `[]`, "object": `{}`, "null": `null`}[kind]
		for _, key := range []string{"entity", "offset", "limit", "filters", "sorts", "countOnly", "fields"} {
			bodies = append(bodies, `{"`+key+`":`+val+`}`)
		}
	}
	for _, field := range []string{"or", "and"} {
		bodies = append(bodies, `{"entity":"b","filters":{"`+field+`":5}}`, `{"entity":"b","filters":{"`+field+`":["x"]}}`)
	}
	var lines goldenio.Lines
	m := &uuidMasker{seen: map[string]string{}}
	for i, b := range bodies {
		runRequests("query-errors", []request{q(fmt.Sprintf("case-%03d", i), bsl(b))}, &lines, m)
	}
	return lines.Write(out, "query_payload_errors.jsonl")
}

var matchDocs = []string{
	`{"title":"Dune","year":1965,"tags":["sf","classic"],"flag":true,"n":"10","d":"2026-01-15T10:00:00+02:00","dd":"2026-01-15","arr":[1,2.5,"3",true,null,[1],{"a":1}],"o":{"p":{"q":"deep"}},"empty":"","zero":0,"neg":-0}`,
	`{"title":"dune messiah","year":"1969","tags":[],"flag":false,"n":10,"d":"2026-01-15","list":[{"name":"x"},{"name":"Y"}],"mixed":["A","b",1e21]}`,
	`{"title":"Emma","flag":"true","n":"abc","d":"not a date","year":null,"tags":"sf","o":{"p":[{"q":"one"},{"q":"two"}]}}`,
	`{"title":"\u00c9t\u00e9 \u0130stanbul","tags":["\u03a3\u03c3"],"n":1.5e-7,"year":1e21,"flag":1}`,
	`{"id":"x1","@entity":"book","_elysiandb_core_owner":"admin","title":"core"}`,
	`{}`,
}

var filterSets = []string{
	`{"title":{"eq":"dune"}}`, `{"title":{"eq":"dune*"}}`, `{"title":{"eq":"*UNE*"}}`, `{"title":{"neq":"dune"}}`, `{"title":{"contains":"une"}}`, `{"title":{"not_contains":"une"}}`,
	`{"title":{"lt":"E"}}`, `{"title":{"gte":"dune"}}`, `{"title":{"all":"dune"}}`, `{"title":{"any":"x,Dune"}}`, `{"title":{"none":"x"}}`, `{"title":{"bogus":"x"}}`,
	`{"year":{"gt":"1960"}}`, `{"year":{"lt":"2"}}`, `{"year":{"eq":"1965"}}`, `{"year":{"eq":"196*"}}`, `{"year":{"gte":"1e21"}}`, `{"year":{"neq":"1965"}}`, `{"year":{"eq":""}}`,
	`{"flag":{"eq":"true"}}`, `{"flag":{"eq":"1"}}`, `{"flag":{"eq":"nope"}}`, `{"flag":{"neq":"true"}}`, `{"flag":{"eq":"TRUE"}}`,
	`{"tags":{"eq":"classic,sf"}}`, `{"tags":{"eq":"sf"}}`, `{"tags":{"any":"sf,x"}}`, `{"tags":{"all":"sf,classic"}}`, `{"tags":{"none":"x"}}`, `{"tags":{"contains":"cla"}}`, `{"tags":{"not_contains":"sf"}}`, `{"tags":{"eq":""}}`,
	`{"arr":{"any":"1,3"}}`, `{"arr":{"all":"2.5,true,<nil>"}}`, `{"arr":{"contains":"[1]"}}`, `{"arr":{"eq":"1,2.5,3,true,<nil>,[1],map[a:1]"}}`, `{"mixed":{"any":"1000000000000000000000"}}`,
	`{"n":{"gt":"9"}}`, `{"n":{"eq":"10"}}`, `{"n":{"lt":"1e-6"}}`, `{"n":{"eq":"abc"}}`,
	`{"d":{"eq":"2026-01-15"}}`, `{"d":{"gt":"2026-01-14T23:00:00Z"}}`, `{"d":{"lt":"2026-01-15T09:00:00Z"}}`, `{"dd":{"eq":"2026-01-15T00:00:00Z"}}`, `{"d":{"gte":"2026-01-15"}}`, `{"d":{"neq":"2026-01-15"}}`,
	`{"o.p.q":{"eq":"deep"}}`, `{"o.p.q":{"eq":"two"}}`, `{"list.name":{"eq":"y"}}`, `{"o..p":{"eq":"x"}}`, `{"":{"eq":""}}`, `{"missing":{"neq":"x"}}`, `{"missing":{"eq":""}}`,
	`{"empty":{"eq":""}}`, `{"zero":{"eq":"0"}}`, `{"neg":{"eq":"0"}}`, `{"neg":{"eq":"-0"}}`, `{"title":{}}`, `{"title":{"eq":"\u00e9t\u00e9 i\u0307stanbul"}}`, `{"tags":{"eq":"\u03c3\u03c3"}}`,
	`{"title":{"eq":"dune","contains":"x"}}`, `{"title":{"gt":"A","lt":"F"}}`, `{"id":{"eq":"x1"}}`, `{"_elysiandb_core_owner":{"eq":"admin"}}`,
}

var searchPatterns = []string{"dune", "*une*", "DUNE", "messiah", "*", "", "deep", "\u00e9t\u00e9", "1965", "true", "admin", "x1", "book", "one*", "*two", "a b", "*\u0130*"}

func genMatcher(out string) error {
	var lines goldenio.Lines
	docs := append([]string(nil), matchDocs...)
	r := rand.New(rand.NewSource(11))
	words := []string{"alpha", "Beta", "gamma", "2026-03-01", "12", "-3.5", "true", "", "\u00c9t\u00e9"}
	for i := 0; i < 200; i++ {
		d := map[string]any{}
		for _, f := range []string{"title", "year", "tags", "flag", "n", "d"} {
			switch r.Intn(5) {
			case 0:
			case 1:
				d[f] = words[r.Intn(len(words))]
			case 2:
				d[f] = float64(r.Intn(3000)) / 2
			case 3:
				d[f] = r.Intn(2) == 0
			default:
				arr := []any{}
				for k := r.Intn(4); k > 0; k-- {
					arr = append(arr, words[r.Intn(len(words))])
				}
				d[f] = arr
			}
		}
		b, _ := json.Marshal(d)
		docs = append(docs, string(b))
	}
	for di, raw := range docs {
		var doc map[string]any
		if err := json.Unmarshal([]byte(raw), &doc); err != nil {
			return fmt.Errorf("matcher doc %d: %w", di, err)
		}
		row := map[string]any{"doc": di, "raw": raw}
		var filters []any
		for _, fs := range filterSets {
			var f map[string]map[string]string
			if err := json.Unmarshal([]byte(fs), &f); err != nil {
				return fmt.Errorf("filter %s: %w", fs, err)
			}
			filters = append(filters, []any{fs, api_storage.FiltersMatchEntity(doc, f), api_storage.StoredFiltersMatchEntity(doc, f)})
		}
		row["filters"] = filters
		var search []any
		for _, p := range searchPatterns {
			matched, ok := api_storage.SearchMatchesBytes([]byte(raw), p)
			search = append(search, []any{p, api_storage.SearchMatchesEntity(doc, p), matched, ok})
		}
		row["search"] = search
		lines.Add(row)
	}
	return lines.Write(out, "matcher_corpus.jsonl")
}

func plannerDoc(i int) map[string]any {
	return map[string]any{
		"id":     fmt.Sprintf("d%05d", i),
		"title":  fmt.Sprintf("Title %03d", i%200),
		"num":    float64(i % 97),
		"status": []string{"open", "closed", "draft", "review"}[i%4],
		"tags":   []any{"red", "green", "blue", "x"}[:i%5%4],
		"date":   time.Unix(1700000000+int64(i)*3600, 0).UTC().Format(time.RFC3339),
		"author": map[string]any{"@entity": "writer", "id": fmt.Sprintf("w%02d", i%23)},
	}
}

func genPlanner(out string) error {
	var lines goldenio.Lines
	for _, size := range []int{100, 3000, 9000} {
		openStore(false, false, false)
		for i := 0; i < 23; i++ {
			api_storage.WriteEntity("writer", map[string]any{"id": fmt.Sprintf("w%02d", i), "name": fmt.Sprintf("Writer %d", i), "country": []string{"fr", "us", "jp"}[i%3]})
		}
		entity := fmt.Sprintf("e%d", size)
		batch := []map[string]any{}
		for i := 0; i < size; i++ {
			batch = append(batch, plannerDoc(i))
			if len(batch) == 500 || i == size-1 {
				api_storage.WriteListOfEntities(entity, batch)
				batch = []map[string]any{}
			}
		}
		lqs := []map[string]any{
			{}, {"limit": 10}, {"offset": 5, "limit": 7}, {"limit": -1}, {"offset": -3},
			{"filters": map[string]map[string]string{"status": {"eq": "open"}}},
			{"filters": map[string]map[string]string{"status": {"eq": "open"}, "num": {"gt": "50"}}, "sort": "num", "asc": false, "limit": 20},
			{"filters": map[string]map[string]string{"title": {"eq": "title 01*"}}},
			{"filters": map[string]map[string]string{"tags": {"any": "red,blue"}}, "count": true},
			{"filters": map[string]map[string]string{"date": {"gte": "2023-11-20"}}, "sort": "date", "asc": true, "limit": 15},
			{"filters": map[string]map[string]string{"author.country": {"eq": "jp"}}, "limit": 25},
			{"search": "*title 1*", "limit": 30},
			{"sort": "title", "asc": true, "offset": 10, "limit": 10},
			{"sorts": []query.Sort{{Field: "status", Ascending: true}, {Field: "num", Ascending: false}}, "limit": 40},
			{"includes": "author", "limit": 5},
			{"count": true},
			{"count": true, "limit": 3},
			{"node": `{"or":[{"status":{"eq":"draft"}},{"num":{"lt":"3"}}]}`, "limit": 50},
			{"node": `{"and":[{"status":{"eq":"open"}},{"or":[{"num":{"eq":"4"}},{"num":{"eq":"8"}}]}]}`},
		}
		for qi, spec := range lqs {
			lq := api_storage.ListQuery{Entity: entity}
			if v, ok := spec["limit"].(int); ok {
				lq.Limit = v
			}
			if v, ok := spec["offset"].(int); ok {
				lq.Offset = v
			}
			if v, ok := spec["filters"].(map[string]map[string]string); ok {
				lq.Filters = v
			}
			if v, ok := spec["sort"].(string); ok {
				lq.SortField = v
				lq.SortAsc = spec["asc"].(bool)
			}
			if v, ok := spec["sorts"].([]query.Sort); ok {
				lq.Sorts = v
			}
			if v, ok := spec["search"].(string); ok {
				lq.Search = v
			}
			if v, ok := spec["includes"].(string); ok {
				lq.Includes = v
			}
			if v, ok := spec["count"].(bool); ok {
				lq.CountOnly = v
			}
			if v, ok := spec["node"].(string); ok {
				var raw map[string]any
				_ = json.Unmarshal([]byte(v), &raw)
				node, err := api_controller.ParseFilterNode(raw)
				if err != nil {
					return err
				}
				lq.Node = &node
			}
			res := api_storage.Plan(lq)
			row := map[string]any{"size": size, "case": qi, "query": describeListQuery(lq), "ids": res.IDs(), "total": res.Total}
			if lq.Node == nil && len(lq.Sorts) == 0 {
				var listed []string
				for _, d := range api_storage.ListEntities(entity, lq.Limit, lq.Offset, lq.SortField, lq.SortAsc, lq.Filters, lq.Search, lq.Includes) {
					id, _ := d["id"].(string)
					listed = append(listed, id)
				}
				row["list_entities"] = listed
			}
			if lq.Node != nil {
				qq := query.Query{Entity: entity, Offset: lq.Offset, Limit: lq.Limit, Filter: *lq.Node}
				docs, err := api_storage.ExecuteQuery(qq)
				var ids []string
				for _, d := range docs {
					id, _ := d["id"].(string)
					ids = append(ids, id)
				}
				row["execute_query"] = ids
				row["execute_query_error"] = err != nil
			}
			lines.Add(row)
		}
		for _, sf := range []struct {
			f   string
			asc bool
		}{{"", true}, {"num", true}, {"num", false}, {"title", true}, {"missing", true}} {
			b, err := api_storage.GetListOfIds(entity, sf.f, sf.asc)
			row := map[string]any{"size": size, "fn": "GetListOfIds", "field": sf.f, "asc": sf.asc, "error": err != nil}
			if err == nil {
				textOrHex(row, "out", b)
			}
			lines.Add(row)
		}
		for _, key := range []string{"api:entity:types:list", "api:entity:" + entity + ":internal:index:id", "api:entity:" + entity + ":internal:index:fields:all", "api:entity:" + entity + ":id:d00001", "api:entity:writer:id:w01", "api:entity:nothing:id:x", "plain-key"} {
			b, err := storage.GetByKey(key)
			row := map[string]any{"size": size, "fn": "GetByKey", "key": key, "error": err != nil}
			if err == nil {
				textOrHex(row, "out", b)
			}
			lines.Add(row)
		}
		closeStore()
	}
	return lines.Write(out, "planner_corpus.jsonl")
}

func describeListQuery(lq api_storage.ListQuery) map[string]any {
	out := map[string]any{"entity": lq.Entity, "limit": lq.Limit, "offset": lq.Offset, "sort_field": lq.SortField, "sort_asc": lq.SortAsc, "search": lq.Search, "includes": lq.Includes, "count_only": lq.CountOnly}
	if lq.Filters != nil {
		out["filters"] = lq.Filters
	}
	if lq.Sorts != nil {
		var s [][]any
		for _, x := range lq.Sorts {
			s = append(s, []any{x.Field, x.Ascending})
		}
		out["sorts"] = s
	}
	if lq.Node != nil {
		out["node"] = describeNode(*lq.Node)
	}
	return out
}

func describeNode(n query.FilterNode) map[string]any {
	out := map[string]any{}
	if n.Or != nil {
		var l []any
		for _, c := range n.Or {
			l = append(l, describeNode(c))
		}
		out["or"] = l
	}
	if n.And != nil {
		var l []any
		for _, c := range n.And {
			l = append(l, describeNode(c))
		}
		out["and"] = l
	}
	if n.Leaf != nil {
		out["leaf"] = n.Leaf
	}
	return out
}

func genSort(out string) error {
	var lines goldenio.Lines
	lists := []string{
		`[{"id":"a","v":3},{"id":"b","v":"3"},{"id":"c"},{"id":"d","v":null},{"id":"e","v":true},{"id":"f","v":"2026-01-01"},{"id":"g","v":1},{"id":"h","v":[1]},{"id":"i","v":{"x":1}},{"id":"j","v":false},{"id":"k","v":"abc"},{"id":"l","v":"2026-01-01T00:00:00Z"},{"id":"m","v":-0},{"id":"n","v":0}]`,
		`[{"id":"1","a":1,"b":"x"},{"id":"2","a":1,"b":"y"},{"id":"3","a":2,"b":"x"},{"id":"4","a":1},{"id":"5","b":"x"},{"id":"6","a":1,"b":"x"}]`,
		`[{"id":"t1","d":"2026-01-01T10:00:00+02:00"},{"id":"t2","d":"2026-01-01T09:00:00Z"},{"id":"t3","d":"2026-01-01"},{"id":"t4","d":"2025-12-31T23:59:59Z"},{"id":"t5","d":"not-a-date"}]`,
		`[{"id":"u1","s":"b"},{"id":"u2","s":"B"},{"id":"u3","s":"\u00e9"},{"id":"u4","s":"e"},{"id":"u5","s":""},{"id":"u6","s":"a"}]`,
		`[{"id":"n1","o":{"p":2}},{"id":"n2","o":{"p":1}},{"id":"n3","o":{}},{"id":"n4"}]`,
	}
	sortSpecs := [][]query.Sort{
		{{Field: "v", Ascending: true}}, {{Field: "v", Ascending: false}}, {{Field: "a", Ascending: true}, {Field: "b", Ascending: false}}, {{Field: "a", Ascending: false}, {Field: "b", Ascending: true}},
		{{Field: "d", Ascending: true}}, {{Field: "d", Ascending: false}}, {{Field: "s", Ascending: true}}, {{Field: "s", Ascending: false}}, {{Field: "o.p", Ascending: true}}, {{Field: "o.p", Ascending: false}},
		{{Field: "missing", Ascending: true}}, {}, {{Field: "id", Ascending: false}},
	}
	for li, l := range lists {
		for si, spec := range sortSpecs {
			var data []map[string]any
			if err := json.Unmarshal([]byte(l), &data); err != nil {
				return err
			}
			api_storage.SortEntities(data, spec)
			var ids []string
			for _, d := range data {
				ids = append(ids, d["id"].(string))
			}
			var s [][]any
			for _, x := range spec {
				s = append(s, []any{x.Field, x.Ascending})
			}
			lines.Add(map[string]any{"list": li, "input": l, "sorts": s, "spec": si, "ids": ids})
		}
	}
	return lines.Write(out, "sort_corpus.jsonl")
}

func genInclude(out string) error {
	openStore(false, false, false)
	defer closeStore()
	api_storage.WriteEntity("person", map[string]any{"id": "p1", "name": "Ann", "friend": map[string]any{"@entity": "person", "id": "p2"}})
	api_storage.WriteEntity("person", map[string]any{"id": "p2", "name": "Bob", "friend": map[string]any{"@entity": "person", "id": "p1"}})
	chain := map[string]any{"id": "c0", "next": map[string]any{"@entity": "chain", "id": "c1"}}
	api_storage.WriteEntity("chain", chain)
	for i := 1; i < 40; i++ {
		api_storage.WriteEntity("chain", map[string]any{"id": fmt.Sprintf("c%d", i), "next": map[string]any{"@entity": "chain", "id": fmt.Sprintf("c%d", i+1)}})
	}
	api_storage.WriteEntity("org", map[string]any{"id": "o1", "name": "Org", "owner": map[string]any{"@entity": "person", "id": "p1"}})
	docs := `[{"id":"d1","author":{"@entity":"person","id":"p1"},"co":[{"@entity":"person","id":"p2"},"text",1,{"plain":true},{"@entity":"person","id":"zz"}],"org":{"@entity":"org","id":"o1"},"core":{"@entity":"_elysiandb_core_user","id":"admin"},"tags":["a","b"],"n":5},` +
		`{"id":"d2","author":{"@entity":"person","id":"missing"},"chain":{"@entity":"chain","id":"c0"}},{"id":"d3"}]`
	includes := []string{"", "all", "author", "author.friend", "author.friend.friend", "co", "org.owner", "org.owner.friend", "chain", "chain.next.next.next", "tags", "n", "core", "author,co", " author , org ", "missing", "all,author"}
	var lines goldenio.Lines
	for _, inc := range includes {
		var data []map[string]any
		if err := json.Unmarshal([]byte(docs), &data); err != nil {
			return err
		}
		res := api_storage.ApplyIncludes(data, inc)
		b, err := json.Marshal(res)
		if err != nil {
			return err
		}
		lines.Add(map[string]any{"fn": "ApplyIncludes", "includes": inc, "out": string(b)})
	}
	filterCases := []map[string]map[string]string{
		{}, {"title": {"eq": "x"}}, {"author.name": {"eq": "x"}}, {"author.friend.name": {"eq": "x"}, "org.owner.name": {"neq": "y"}}, {"a.b": {"eq": "1"}, "a.c": {"eq": "2"}, "d": {"eq": "3"}}, {".x": {"eq": "1"}, "x.": {"eq": "1"}},
	}
	for _, f := range filterCases {
		auto := api_storage.ExtractAutoIncludes(f)
		parts := strings.Split(auto, ",")
		sort.Strings(parts)
		lines.Add(map[string]any{"fn": "ExtractAutoIncludes", "filters": f, "out_set": parts})
	}
	for _, p := range [][2]string{{"", ""}, {"a", ""}, {"", "b"}, {"a", "b"}, {"a,b", "b,c"}, {"all", "a"}, {"a", "all"}, {" a ", "a"}, {"a.b", "a"}, {"a,,b", ""}} {
		lines.Add(map[string]any{"fn": "MergeIncludes", "a": p[0], "b": p[1], "out": api_storage.MergeIncludes(p[0], p[1])})
	}
	return lines.Write(out, "include_corpus.jsonl")
}

func genWrite(out string) error {
	var lines goldenio.Lines
	m := &uuidMasker{seen: map[string]string{}}
	bodies := []string{
		`{"id":"w1","author":{"@entity":"person","id":"p1","name":"Ann"}}`,
		`{"id":"w2","author":{"@entity":"person","name":"No id"}}`,
		`{"id":"w3","author":{"@entity":"person","id":"p2"}}`,
		`{"id":"w4","co":[{"@entity":"person","id":"p3","name":"C"},{"@entity":"person","name":"D"},"x",{"plain":1}]}`,
		`{"id":"w5","nested":{"deep":{"@entity":"person","id":"p4","name":"Deep"}}}`,
		`{"id":"w6","author":{"@entity":"person","id":"p5","pet":{"@entity":"pet","id":"pt1","name":"Rex"}}}`,
		`{"id":"w7","bad":{"@entity":"","id":"x"},"num":{"@entity":5}}`,
		`{"title":"no id at all"}`,
	}
	openStore(true, false, false)
	for i, b := range bodies {
		var data map[string]any
		if err := json.Unmarshal([]byte(b), &data); err != nil {
			return err
		}
		subs := api_storage.ExtractSubEntities("w", data)
		sb, _ := json.Marshal(subs)
		db, _ := json.Marshal(data)
		lines.Add(map[string]any{"fn": "ExtractSubEntities", "case": i, "in": b, "subs": string(m.mask(sb)), "doc_after": string(m.mask(db))})
	}
	closeStore()
	openStore(true, false, false)
	write := func(name string, fn func() any) {
		res := fn()
		rb, _ := json.Marshal(res)
		lines.Add(map[string]any{"fn": name, "result": string(m.mask(rb)), "stored_w": storedEntity("w", m), "stored_person": storedEntity("person", m)})
	}
	for i, b := range bodies {
		var data map[string]any
		_ = json.Unmarshal([]byte(b), &data)
		write(fmt.Sprintf("WriteEntity#%d", i), func() any { return api_storage.WriteEntity("w", data) })
	}
	write("UpdateEntityById", func() any {
		return api_storage.UpdateEntityById("w", "w1", map[string]any{"title": "T", "author": map[string]any{"@entity": "person", "id": "p1", "name": "Ann B"}})
	})
	write("UpdateEntityById-missing", func() any { return api_storage.UpdateEntityById("w", "zz", map[string]any{"x": 1.0}) })
	write("WriteListOfEntities", func() any {
		return api_storage.WriteListOfEntities("w", []map[string]any{{"id": "l1", "n": 1.0}, {"n": 2.0}, {"id": "l1", "n": 3.0}})
	})
	write("UpdateListOfEntities", func() any {
		return api_storage.UpdateListOfEntities("w", []map[string]any{{"id": "l1", "n": 4.0}, {"id": "zz"}, {"n": 5.0}})
	})
	write("CommitBatch", func() any {
		err := api_storage.CommitBatch([]api_storage.BatchOperation{
			{Kind: "create", Entity: "w", ID: "bt1", Data: map[string]any{"id": "bt1", "v": 1.0}},
			{Kind: "update", Entity: "w", ID: "l1", Data: map[string]any{"v": 2.0}},
			{Kind: "delete", Entity: "w", ID: "w7"},
		})
		return map[string]any{"error": err != nil}
	})
	closeStore()
	openStore(true, false, false)
	api_storage.WriteEntity("person", map[string]any{"id": "p1", "name": "Ann"})
	api_storage.WriteEntity("team", map[string]any{"id": "t1", "lead": map[string]any{"@entity": "person", "id": "p1"}, "members": []any{map[string]any{"@entity": "person", "id": "p1"}}, "title": "x"})
	migrations := []string{`[{"set":{"touched":true}}]`, `[{"set":{"title.sub":"x"}}]`, `[{"set":{"a.b.c":1}}]`, `[{"set":[{"z":1},{"y":2}]}]`, `[{"unknown":{"x":1}},{"set":{"never":1}}]`, `[]`, `[null]`}
	for _, mq := range migrations {
		queries := api_storage.ParseMigrationQuery(mq, "team")
		qb, _ := json.Marshal(queries)
		row := map[string]any{"fn": "Migration", "body": mq, "queries": string(qb)}
		func() {
			defer func() {
				if r := recover(); r != nil {
					row["panic"] = true
				}
			}()
			n, err := api_storage.ExecuteMigrationsCount(queries)
			row["count"] = n
			if err != nil {
				row["error"] = err.Error()
			}
		}()
		row["stored_team"] = storedEntity("team", m)
		row["stored_person"] = storedEntity("person", m)
		if b, ok := api_storage.ReadEntityBytesById("_elysiandb_core_schema", "team"); ok {
			row["schema_document"] = string(b)
		}
		lines.Add(row)
	}
	closeStore()
	return lines.Write(out, "write_corpus.jsonl")
}

func sortedErrors(errs []schema.ValidationError) [][]string {
	out := [][]string{}
	for _, e := range errs {
		out = append(out, []string{e.Field, e.Message})
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i][0] != out[j][0] {
			return out[i][0] < out[j][0]
		}
		return out[i][1] < out[j][1]
	})
	return out
}

func genSchema(out string) error {
	var lines goldenio.Lines
	docs := []string{
		`{"id":"1","s":"x","n":1,"b":true,"nil":null,"a":["x"],"ae":[],"o":{"k":"v","n":{"d":1}},"mixed":[1,"x"]}`,
		`{"id":"2","s":5,"n":"x","b":"true","a":"x","o":[],"extra":1}`,
		`{"id":"3"}`,
		`{"id":"4","o":{"k":1,"n":null},"a":[{"x":1}]}`,
	}
	var parsed []map[string]any
	for _, d := range docs {
		var m map[string]any
		if err := json.Unmarshal([]byte(d), &m); err != nil {
			return err
		}
		parsed = append(parsed, m)
	}
	var analyzed []map[string]any
	for i, d := range parsed {
		a := schema.AnalyzeEntitySchema("ent", d)
		ab, _ := json.Marshal(a)
		analyzed = append(analyzed, a)
		lines.Add(map[string]any{"fn": "AnalyzeEntitySchema", "doc": docs[i], "out": string(ab)})
	}
	for i := range parsed {
		for j := range analyzed {
			merged := schema.MergeAnalyzedSchema("ent", analyzed[j], parsed[i])
			mb, _ := json.Marshal(merged)
			lines.Add(map[string]any{"fn": "MergeAnalyzedSchema", "existing": j, "doc": docs[i], "out": string(mb)})
		}
	}
	manual := []string{
		`{"title":{"type":"string","required":true},"meta":{"type":"object","fields":{"k":{"type":"string","required":true}}},"tags":{"type":"array"},"n":{"type":"number"},"ok":{"type":"boolean"}}`,
		`{"title":{"type":"string"},"bad":"x","num":5,"nested":{"type":"object","fields":"x"}}`,
		`{}`,
	}
	for _, mm := range manual {
		var raw map[string]any
		if err := json.Unmarshal([]byte(mm), &raw); err != nil {
			return err
		}
		fields := schema.MapToFields(raw)
		back := schema.FieldsToMap(fields)
		bb, _ := json.Marshal(back)
		lines.Add(map[string]any{"fn": "MapToFields+FieldsToMap", "in": mm, "out": string(bb)})
	}
	schemas := []string{
		`{"id":"ent","fields":{"title":{"name":"title","type":"string","required":true},"meta":{"name":"meta","type":"object","required":false,"fields":{"k":{"name":"k","type":"string","required":true}}},"n":{"name":"n","type":"number","required":false}}}`,
		`{"id":"ent","_manual":true,"fields":{"title":{"name":"title","type":"string","required":true}}}`,
	}
	targets := []string{`{"title":"x","n":1}`, `{"n":"x"}`, `{"title":null}`, `{"title":"x","meta":{}}`, `{"title":"x","meta":{"k":1}}`, `{"title":"x","extra":true}`, `{"title":5,"n":"y","extra":1,"meta":"z"}`}
	for _, strict := range []bool{false, true} {
		globals.SetConfig(&configuration.Config{Api: configuration.ApiConfig{Schema: configuration.ApiSchemaConfig{Enabled: true, Strict: strict}}})
		for si, sc := range schemas {
			var s map[string]any
			if err := json.Unmarshal([]byte(sc), &s); err != nil {
				return err
			}
			for _, tg := range targets {
				var d map[string]any
				_ = json.Unmarshal([]byte(tg), &d)
				lines.Add(map[string]any{"fn": "ValidateEntity", "strict": strict, "schema": si, "manual": schema.IsManualSchema("ent", s), "doc": tg, "errors": sortedErrors(schema.ValidateEntity("ent", d, s))})
				existing := map[string]any{"title": "old", "n": 1.0}
				lines.Add(map[string]any{"fn": "ValidateEntityUpdate", "strict": strict, "schema": si, "existing": existing, "updates": tg, "errors": sortedErrors(schema.ValidateEntityUpdate("ent", existing, d, s))})
			}
		}
	}
	return lines.Write(out, "schema_corpus.jsonl")
}

func genScalars(out string) error {
	var lines goldenio.Lines
	for _, s := range []string{"2026-01-15", "2026-01-15T10:00:00Z", "2026-01-15T10:00:00+02:00", "2026-1-15", "2026-01-15 10:00:00", "2026-01-15T10:00:00,5Z", "2026-01-15T24:00:00Z", "x", "2026-13-01", "2026-01-15T10:00:00.123456789123Z"} {
		t, ok, dateOnly := docpath.ParseDate(s)
		row := map[string]any{"fn": "ParseDate", "in": s, "ok": ok, "date_only": dateOnly}
		if ok {
			row["unix"] = t.Unix()
			row["nanos"] = t.Nanosecond()
			row["day_unix"] = t.Truncate(24 * time.Hour).Unix()
		}
		st, sok := docpath.ParseSortDate(s)
		row["sort_ok"] = sok
		if sok {
			row["sort_unix"] = st.Unix()
		}
		lines.Add(row)
	}
	for _, g := range [][2]string{{"dune*", "Dune Messiah"}, {"*UNE", "dune"}, {"d?ne", "dune"}, {"a\\*", "a*"}, {"", ""}, {"*", "\xff"}, {"\u00c9*", "\u00e9t\u00e9"}} {
		lines.Add(map[string]any{"fn": "MatchGlob", "pattern": g[0], "subject": g[1], "out": docpath.MatchGlob(g[0], g[1])})
	}
	for _, s := range []string{"1", "1.5", "-0", "1e21", "1e-7", "0x10", "0x1p-2", "1_000", "inf", "-Inf", "nan", "NaN", "1e400", "-1e400", "1e-400", " 1", "1 ", "", ".5", "5.", "+5", "99999999999999999999", "1,5"} {
		f, err := strconv.ParseFloat(s, 64)
		row := map[string]any{"fn": "ParseFloat", "in": s, "bits": fmt.Sprintf("%016x", math.Float64bits(f))}
		var ne *strconv.NumError
		if errors.As(err, &ne) {
			if errors.Is(ne.Err, strconv.ErrRange) {
				row["error"] = "range"
			} else {
				row["error"] = "syntax"
			}
		}
		lines.Add(row)
	}
	for _, f := range []float64{0, math.Copysign(0, -1), 1, 1.5, 1e20, 1e21, 1e-6, 1e-7, 123456789, 0.1, 1 << 53, math.MaxFloat64, math.SmallestNonzeroFloat64} {
		lines.Add(map[string]any{"fn": "FormatFloat", "bits": fmt.Sprintf("%016x", math.Float64bits(f)), "f": strconv.FormatFloat(f, 'f', -1, 64), "g": strconv.FormatFloat(f, 'g', -1, 64)})
	}
	for _, j := range []string{`[1,"a",null,true,[2,[3]],{"b":1,"a":[1]}]`, `{"z":1,"a":{"c":[],"b":null}}`, `[]`, `{}`, `[1e21,1e-7,-0]`} {
		var v any
		_ = json.Unmarshal([]byte(j), &v)
		lines.Add(map[string]any{"fn": "Sprintf_v", "in": j, "out": fmt.Sprintf("%v", v)})
	}
	for _, s := range []string{"ABC", "\u0130stanbul", "\u03a3\u03c3\u03c2", "\u1e9e", "\u01c5", "\u212a", "\xffA", "  x  ", "\u00a0x\u00a0", "\u0085x", "\u2028x\u2029", "\u3000x", "\ufeffx", "\u180ex", "\u200bx", "\tx\n"} {
		row := map[string]any{"fn": "ToLower+TrimSpace", "in": hs(s), "lower": hs(strings.ToLower(s)), "trim": hs(strings.TrimSpace(s))}
		lines.Add(row)
	}
	for _, s := range []string{"dune", "*dune*", "DUNE", "*", "", "a*b", "\u00c9t\u00e9", "  x ", "\xff", "**", "a\\*"} {
		lines.Add(map[string]any{"fn": "SearchPattern", "in": hs(s), "pattern": hs(api_storage.SearchPattern(s)), "lowered": hs(api_storage.LoweredSearchPattern(s))})
	}
	return lines.Write(out, "scalar_vectors.jsonl")
}

func genCacheKeys(out string) error {
	var lines goldenio.Lines
	inputs := []struct {
		entity, sortField, fields, search, includes, user string
		limit, offset                                     int
		asc, count                                        bool
		filters                                           map[string]map[string]string
	}{
		{entity: "book"},
		{entity: "book", limit: 10, offset: 5, sortField: "year", asc: true},
		{entity: "book", limit: 10, offset: 5, sortField: "year", asc: false},
		{entity: "book", filters: map[string]map[string]string{"title": {"eq": "dune", "neq": "x"}, "year": {"gt": "1"}}},
		{entity: "book", fields: "title,year", search: "*x*", includes: "author", count: true, user: "alice"},
		{entity: "b|ok", sortField: "a|b", fields: "|", search: "||", includes: "|", user: "|"},
		{entity: "\xff", filters: map[string]map[string]string{"\xff": {"\xfe": "\xfd"}}},
	}
	for i, in := range inputs {
		h := cache.HashQuery(in.entity, in.limit, in.offset, in.sortField, in.asc, in.filters, in.fields, in.search, in.includes, in.count, in.user)
		lines.Add(map[string]any{"fn": "HashQuery", "case": i, "entity": hs(in.entity), "limit": in.limit, "offset": in.offset, "sort_field": hs(in.sortField), "asc": in.asc, "filters": in.filters, "fields": hs(in.fields), "search": hs(in.search), "includes": hs(in.includes), "count": in.count, "user": hs(in.user), "hex": hex.EncodeToString(h)})
	}
	payloads := []string{
		`{"entity":"book"}`, `{"entity":"book","limit":5,"offset":1,"countOnly":true,"fields":"title"}`,
		`{"entity":"book","filters":{"or":[{"a":{"eq":1}},{"b":{"eq":"x"}}]},"sorts":{"year":"desc","title":"asc"}}`,
		`{"entity":"book","sorts":{"title":"asc","year":"desc"}}`, `{"entity":"book","filters":{"z":{"eq":true},"a":{"gt":1e21}}}`,
	}
	for _, p := range payloads {
		var payload api_controller.QueryPayload
		if err := json.Unmarshal([]byte(p), &payload); err != nil {
			return err
		}
		for _, user := range []string{"", "alice"} {
			h := sha256.New()
			h.Write([]byte(payload.Entity))
			h.Write(payload.Hash())
			h.Write([]byte(user))
			lines.Add(map[string]any{"fn": "QueryHash", "payload": p, "user": user, "payload_hash": hex.EncodeToString(payload.Hash()), "hex": hex.EncodeToString(h.Sum(nil))})
		}
	}
	openStore(false, false, false)
	versions := func(step string) {
		lines.Add(map[string]any{"fn": "entityVersion", "step": step, "store": storeCounter, "book": cache.VersionOf("book"), "none": cache.VersionOf("none")})
	}
	versions("empty")
	api_storage.WriteEntity("book", map[string]any{"id": "b1"})
	versions("one-write")
	for i := 0; i < 40; i++ {
		api_storage.WriteEntity("book", map[string]any{"id": fmt.Sprintf("x%d", i)})
	}
	versions("forty-one-writes")
	api_storage.DeleteAllEntities("book")
	versions("after-reset")
	closeStore()
	return lines.Write(out, "cache_keys.jsonl")
}

func validationErrors(body []byte) ([][]string, bool) {
	var list []map[string]any
	if err := json.Unmarshal(body, &list); err != nil || len(list) < 2 {
		return nil, false
	}
	var out [][]string
	for _, e := range list {
		f, ok1 := e["Field"].(string)
		msg, ok2 := e["Message"].(string)
		if len(e) != 2 || !ok1 || !ok2 {
			return nil, false
		}
		out = append(out, []string{f, msg})
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i][0] != out[j][0] {
			return out[i][0] < out[j][0]
		}
		return out[i][1] < out[j][1]
	})
	return out, true
}
