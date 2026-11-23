package main

import (
	"bytes"
	"fmt"
	"mime/multipart"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync/atomic"

	"github.com/taymour/elysiandb/internal/storage"
)

func config(auth, extra string) string {
	return configWith(auth, "", "", extra)
}

func configWith(auth, httpExtra, apiExtra, extra string) string {
	return `store:
  folder: <DATA>
  shards: 8
  flushIntervalSeconds: 0
  crashRecovery: { enabled: true, maxLogMB: 100 }
server:
  http: { enabled: true, host: 127.0.0.1, port: 8089` + httpExtra + ` }
  tcp:  { enabled: false, host: 127.0.0.1, port: 8088 }
log:
  level: info
  format: text
  accessLog: true
  bufferLines: 500
stats:
  enabled: true
security:
  authentication:
` + auth + `api:
  index:
    workers: 1
  schema:
    enabled: true
    strict: false
  cache:
    enabled: true
    cleanupIntervalSeconds: 10
  hooks:
    enabled: true
` + apiExtra + `adminui:
  enabled: true
monitoring:
  prometheus:
    enabled: true
` + extra
}

const authOff = "    enabled: false\n"

func archive() string {
	b, err := os.ReadFile(filepath.Join("test", "compat", "testdata", "archives", "v0.2.4.tar.gz"))
	if err != nil {
		panic(err)
	}
	return string(b)
}

func multipartBody(boundary string, data string) string {
	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	_ = w.SetBoundary(boundary)
	part, _ := w.CreateFormFile("file", "backup.tar.gz")
	_, _ = part.Write([]byte(data))
	_ = w.Close()
	return buf.String()
}

func chunkedBody(data string, size int) string {
	var b strings.Builder
	for len(data) > 0 {
		n := min(size, len(data))
		b.WriteString(strconv.FormatInt(int64(n), 16) + "\r\n" + data[:n] + "\r\n")
		data = data[n:]
	}
	b.WriteString("0\r\n\r\n")
	return b.String()
}

func jreq(slug, method, target, body string, headers ...string) tcase {
	return simple(slug, method, target, append([]string{"Content-Type: application/json"}, headers...), body)
}

func routeCases(auth []string) []tcase {
	h := func(extra ...string) []string { return append(append([]string{}, auth...), extra...) }
	cases := []tcase{
		simple("health", "GET", "/health", h(), ""),
		{slug: "health-head", steps: []step{{data: reqClose("HEAD", "/health", h(), "")}}, read: closeRead, compare: "exact", head: true},
		simple("kv-put", "PUT", "/kv/foo", h(), "bar"),
		simple("kv-put-ttl", "PUT", "/kv/ttl?ttl=3600", h(), "short"),
		simple("kv-get", "GET", "/kv/foo", h(), ""),
		simple("kv-put-escaped", "PUT", "/kv/a%2Fb%20c", h(), "esc"),
		simple("kv-get-escaped", "GET", "/kv/a%2Fb%20c", h(), ""),
		simple("kv-get-unescaped", "GET", "/kv/a/b c", h(), ""),
		simple("kv-mget", "GET", "/kv/mget?keys=foo,a%2Fb%20c,missing", h(), ""),
		simple("kv-put-mget", "PUT", "/kv/mget", h(), "shadow"),
		simple("kv-get-missing", "GET", "/kv/missing", h(), ""),
		simple("kv-delete", "DELETE", "/kv/foo", h(), ""),
		simple("kv-get-deleted", "GET", "/kv/foo", h(), ""),
		simple("save", "POST", "/save", h(), ""),
		shape(simple("stats", "GET", "/stats", h(), "")),
		simple("config", "GET", "/config", h(), ""),
		jreq("create", "POST", "/api/book", `{"id":"b1","title":"Dune","year":1965,"author":{"@entity":"person","id":"p1","name":"Frank"}}`, h()...),
		jreq("create-no-id", "POST", "/api/book", `{"title":"Anon"}`, h()...),
		jreq("create-list", "POST", "/api/book", `[{"id":"b2","title":"Emma"},{"id":"b3","title":"Ulysses","year":1922}]`, h()...),
		jreq("create-invalid-json", "POST", "/api/book", `{"title":`, h()...),
		simple("list", "GET", "/api/book", h(), ""),
		simple("list-again-cached", "GET", "/api/book", h(), ""),
		simple("list-filter-encoded", "GET", "/api/book?filter%5Btitle%5D=d%75ne*&sort%5Btitle%5D=desc", h(), ""),
		simple("list-includes", "GET", "/api/book?includes=author&fields=title,author.name", h(), ""),
		simple("get", "GET", "/api/book/b1", h(), ""),
		simple("get-missing", "GET", "/api/book/zz", h(), ""),
		simple("count", "GET", "/api/book/count", h(), ""),
		simple("exists", "GET", "/api/book/b1/exists", h(), ""),
		simple("exists-missing", "GET", "/api/book/zz/exists", h(), ""),
		jreq("update", "PUT", "/api/book/b1", `{"year":1966}`, h()...),
		jreq("update-list", "PUT", "/api/book", `[{"id":"b2","year":1815}]`, h()...),
		jreq("query", "POST", "/api/query", `{"entity":"book","filters":{"title":{"eq":"dune"}},"sorts":{"year":"desc"}}`, h()...),
		func() tcase {
			c := jreq("query-decode-error", "POST", "/api/query", `{"entity":"book","limit":"x"}`, h()...)
			c.compare = "d27"
			return c
		}(),
		func() tcase {
			c := jreq("query-syntax-error", "POST", "/api/query", `not json`, h()...)
			c.compare = "d27"
			return c
		}(),
		jreq("query-filter-error", "POST", "/api/query", `{"entity":"book","filters":{"or":5}}`, h()...),
		jreq("migrate", "POST", "/api/book/migrate", `[{"set":{"genre":"novel"}}]`, h()...),
		simple("types", "GET", "/api/entity/types", h(), ""),
		simple("types-names", "GET", "/api/entity/types/name", h(), ""),
		jreq("create-type", "POST", "/api/movie/create", `{"fields":{"title":{"type":"string","required":true}}}`, h()...),
		simple("get-schema", "GET", "/api/movie/schema", h(), ""),
		jreq("put-schema", "PUT", "/api/movie/schema", `{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"}}}`, h()...),
		jreq("create-schema-error", "POST", "/api/movie", `{"id":"m1","title":5}`, h()...),
		withCapture(jreq("tx-begin", "POST", "/api/tx/begin", ``, h()...), "tx", `"transaction_id":"([^"]+)"`),
		jreq("tx-write", "POST", "/api/tx/{{tx}}/entity/book", `{"id":"t1","title":"In tx"}`, h()...),
		jreq("tx-update", "PUT", "/api/tx/{{tx}}/entity/book/b2", `{"title":"Emma (tx)"}`, h()...),
		simple("tx-delete", "DELETE", "/api/tx/{{tx}}/entity/book/b3", h(), ""),
		simple("tx-commit", "POST", "/api/tx/{{tx}}/commit", h(), ""),
		withCapture(jreq("tx-begin-2", "POST", "/api/tx/begin", ``, h()...), "tx2", `"transaction_id":"([^"]+)"`),
		simple("tx-rollback", "POST", "/api/tx/{{tx2}}/rollback", h(), ""),
		simple("tx-unknown", "POST", "/api/tx/nope/commit", h(), ""),
		withCapture(jreq("hook-create", "POST", "/api/hook/book", `{"name":"upper","event":"post_read","language":"javascript","script":"function postRead(ctx){ ctx.entity.upper = (ctx.entity.title || '').toUpperCase(); }","priority":1,"enabled":true}`, h()...), "hook", `"id":"([^"]+)"`),
		simple("hook-list", "GET", "/api/hook/book", h(), ""),
		simple("hook-get", "GET", "/api/hook/id/{{hook}}", h(), ""),
		simple("list-with-hook", "GET", "/api/book?limit=2", h(), ""),
		jreq("hook-update", "PUT", "/api/hook/id/{{hook}}", `{"enabled":false}`, h()...),
		simple("hook-delete", "DELETE", "/api/hook/book/{{hook}}", h(), ""),
		simple("hook-get-missing", "GET", "/api/hook/id/nope", h(), ""),
		simple("export", "GET", "/api/export", h(), ""),
		jreq("import", "POST", "/api/import", `{"imported":[{"id":"i1","v":1}]}`, h()...),
		{slug: "backup-head", steps: []step{{data: reqClose("GET", "/api/backup", h(), "")}}, read: closeRead, compare: "head-only"},
		shape(simple("monitoring-overview", "GET", "/monitoring/overview", h(), "")),
		shape(simple("monitoring-series", "GET", "/monitoring/series?range=15m", h(), "")),
		shape(simple("monitoring-details", "GET", "/monitoring/details", h(), "")),
		shape(simple("monitoring-logs", "GET", "/monitoring/logs?limit=5", h(), "")),
		{slug: "monitoring-logs-download", steps: []step{{data: reqClose("GET", "/monitoring/logs/download", h(), "")}}, read: closeRead, compare: "head-only"},
		simple("monitoring-level-get", "GET", "/monitoring/logs/level", h(), ""),
		jreq("monitoring-level-put", "PUT", "/monitoring/logs/level", `{"level":"debug","minutes":5}`, h()...),
		jreq("monitoring-level-put-bad", "PUT", "/monitoring/logs/level", `{"level":"loud"}`, h()...),
		{slug: "metrics", steps: []step{{data: reqClose("GET", "/metrics", h(), "")}}, read: closeRead, compare: "head-only"},
		simple("delete", "DELETE", "/api/book/b2", h(), ""),
		simple("destroy", "DELETE", "/api/movie", h(), ""),
	}
	return cases
}

func restoreCases(auth []string) []tcase {
	arch := archive()
	boundary := "GOLDENBOUNDARY"
	mp := multipartBody(boundary, arch)
	h := func(extra ...string) []string { return append(append([]string{}, auth...), extra...) }
	return []tcase{
		simple("restore-length", "POST", "/api/restore", h("Content-Type: application/gzip"), arch),
		{slug: "restore-chunked", steps: []step{{data: "POST /api/restore HTTP/1.1\r\nHost: h\r\nConnection: close\r\n" + strings.Join(h("Content-Type: application/gzip"), "\r\n") + "\r\nTransfer-Encoding: chunked\r\n\r\n" + chunkedBody(arch, 4096)}}, read: closeRead, compare: "exact"},
		{slug: "restore-multipart-expect", steps: []step{
			{data: "POST /api/restore HTTP/1.1\r\nHost: h\r\nConnection: close\r\n" + strings.Join(h("Content-Type: multipart/form-data; boundary="+boundary, "Expect: 100-continue"), "\r\n") + "\r\nContent-Length: " + strconv.Itoa(len(mp)) + "\r\n\r\n"},
			{data: mp, pause: 300},
		}, read: closeRead, compare: "exact"},
		func() tcase {
			c := simple("restore-not-an-archive", "POST", "/api/restore", h("Content-Type: application/gzip"), "not a gzip stream")
			c.compare = "d27"
			return c
		}(),
		simple("restore-multipart-malformed", "POST", "/api/restore", h("Content-Type: multipart/form-data; boundary=XYZ"), "--XYZ\r\nbroken"),
		simple("list-after-restore", "GET", "/api/book", h(), ""),
	}
}

func routerCases() []tcase {
	return []tcase{
		simple("not-found", "GET", "/nope", nil, ""),
		simple("method-not-allowed", "DELETE", "/health", nil, ""),
		simple("patch", "PATCH", "/health", nil, ""),
		simple("trace", "TRACE", "/health", nil, ""),
		rawCase("connect", "CONNECT h:80 HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n", closeRead, "exact"),
		simple("unknown-method", "FOO", "/health", nil, ""),
		simple("options-no-origin", "OPTIONS", "/health", nil, ""),
		simple("options-origin", "OPTIONS", "/api/book", []string{"Origin: http://elsewhere.test", "Access-Control-Request-Method: GET"}, ""),
		simple("trailing-slash-get", "GET", "/health/", nil, ""),
		simple("trailing-slash-post", "POST", "/save/", nil, ""),
		simple("case-redirect", "GET", "/HEALTH", nil, ""),
		simple("double-slash", "GET", "//health", nil, ""),
		simple("dot-segment", "GET", "/./health", nil, ""),
		simple("dot-dot", "GET", "/api/../health", nil, ""),
		rawCase("http10-no-host-redirect", "GET /health/ HTTP/1.0\r\n\r\n", closeRead, "exact"),
		simple("static-vs-param-types", "GET", "/api/entity/types", nil, ""),
		simple("static-vs-param-count", "GET", "/api/book/count", nil, ""),
		simple("param-entity-id", "GET", "/api/entity/x", nil, ""),
		simple("admin-redirect", "GET", "/admin", nil, ""),
		simple("admin-root", "GET", "/admin/", nil, ""),
		simple("admin-gitkeep", "GET", "/admin/.gitkeep", nil, ""),
		simple("admin-spa", "GET", "/admin/users/42", nil, ""),
		simple("admin-asset-missing", "GET", "/admin/assets/app.js", nil, ""),
		simple("admin-dotdot-panic", "GET", "/admin/../kv", nil, ""),
		simple("encoded-slash", "GET", "/api/book%2Fb1", nil, ""),
		simple("percent-invalid", "GET", "/api/book?filter[title]=%zz", nil, ""),
	}
}

func serverCases() []tcase {
	big := strings.Repeat("a", 4<<20)
	bigger := big + "a"
	head60 := strings.Repeat("x", 60<<10)
	head70 := strings.Repeat("x", 70<<10)
	return []tcase{
		rawCase("garbage-request-line", "BLAH\r\n\r\n", closeRead, "d27"),
		rawCase("garbage-bytes", "\x00\x01\x02\r\n\r\n", closeRead, "d27"),
		rawCase("duplicate-content-length", "POST /api/query HTTP/1.1\r\nHost: h\r\nContent-Length: 2\r\nContent-Length: 3\r\nConnection: close\r\n\r\n{}x", closeRead, "d27"),
		simple("body-4mib-exact", "PUT", "/kv/big", nil, big),
		func() tcase {
			c := simple("body-4mib-plus-one", "PUT", "/kv/big", nil, bigger)
			c.compare = "d27"
			return c
		}(),
		rawCase("body-4mib-chunked", "PUT /kv/bigc HTTP/1.1\r\nHost: h\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n"+chunkedBody(big, 1<<20), closeRead, "exact"),
		rawCase("body-4mib-plus-one-chunked", "PUT /kv/bigc HTTP/1.1\r\nHost: h\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n"+chunkedBody(bigger, 1<<20), closeRead, "d27"),
		simple("head-60kib", "GET", "/health", []string{"X-Big: " + head60}, ""),
		func() tcase {
			c := simple("head-70kib", "GET", "/health", []string{"X-Big: " + head70}, "")
			c.compare = "d27"
			return c
		}(),
		{slug: "expect-continue", steps: []step{
			{data: "POST /api/book HTTP/1.1\r\nHost: h\r\nConnection: close\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nContent-Length: 27\r\n\r\n"},
			{data: `{"id":"e1","title":"Later"}`, pause: 200},
		}, read: closeRead, compare: "exact"},
		rawCase("http10-close", "GET /health HTTP/1.0\r\nHost: h\r\n\r\n", closeRead, "exact"),
		rawCase("http10-keepalive", "GET /health HTTP/1.0\r\nHost: h\r\nConnection: keep-alive\r\n\r\n", quietRead(800), "exact"),
		rawCase("connection-close", "GET /health HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n", closeRead, "exact"),
		rawCase("http2-token", "GET /health HTTP/2.0\r\nHost: h\r\n\r\n", quietRead(800), "exact"),
		rawCase("pipelined", "GET /health HTTP/1.1\r\nHost: h\r\n\r\nGET /api/book/count HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n", closeRead, "exact"),
		rawCase("chunked-request", "POST /api/book HTTP/1.1\r\nHost: h\r\nConnection: close\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n"+chunkedBody(`{"id":"c1","title":"Chunked"}`, 7), closeRead, "exact"),
		rawCase("absolute-form", "GET http://h/health HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n", closeRead, "exact"),
		simple("client-internal-body-length", "GET", "/health", []string{"X-Elysian-Internal-Body-Length: 5"}, ""),
		simple("request-id-valid", "GET", "/health", []string{"X-Request-Id: client-supplied-1"}, ""),
		simple("request-id-invalid", "GET", "/health", []string{"X-Request-Id: bad id!"}, ""),
		simple("keepalive-then-close", "GET", "/api/book?limit=1", nil, ""),
		simple("large-list", "GET", "/api/big", nil, ""),
		simple("large-list-paged", "GET", "/api/big?limit=100&offset=50", nil, ""),
		jreq("large-query", "POST", "/api/query", `{"entity":"big"}`),
		{slug: "large-export", steps: []step{{data: reqClose("GET", "/api/export", nil, "")}}, read: closeRead, compare: "exact"},
	}
}

func seedBig(addr string) {
	var docs []string
	for i := 0; i < 2600; i++ {
		docs = append(docs, fmt.Sprintf(`{"id":"big%04d","text":"%s","n":%d}`, i, strings.Repeat("lorem ipsum ", 40), i))
	}
	for start := 0; start < len(docs); start += 500 {
		end := min(start+500, len(docs))
		postJSON(addr, "/api/big", "["+strings.Join(docs[start:end], ",")+"]")
	}
}

var freeSpace atomic.Int64

func scenarios() []scenario {
	login := func(user, pass string) tcase {
		return withCapture(jreq("login-"+user, "POST", "/api/security/login", jsonBody(map[string]string{"username": user, "password": pass})), "sid", `edb_session=([0-9a-f]+)`)
	}
	cookie := []string{"Cookie: edb_session={{sid}}"}
	return []scenario{
		{name: "off", config: config(authOff, ""), seed: seedBig, cases: func() []tcase {
			var c []tcase
			c = append(c, routeCases(nil)...)
			c = append(c, routerCases()...)
			c = append(c, serverCases()...)
			c = append(c, restoreCases(nil)...)
			c = append(c, simple("reset", "POST", "/reset", nil, ""), simple("list-after-reset", "GET", "/api/book", nil, ""))
			return c
		}},
		{name: "user", config: config("    enabled: true\n    mode: user\n", ""), cases: func() []tcase {
			c := []tcase{
				simple("no-credentials", "GET", "/api/book", nil, ""),
				simple("health-open", "GET", "/health", nil, ""),
				jreq("login-bad", "POST", "/api/security/login", `{"username":"admin","password":"wrong"}`),
				jreq("login-malformed", "POST", "/api/security/login", `{"username":`),
				login("admin", "admin"),
				simple("me", "GET", "/api/security/me", cookie, ""),
				jreq("user-create", "POST", "/api/security/user", `{"username":"ann","password":"secret-1","role":"user"}`, cookie...),
				jreq("user-create-duplicate", "POST", "/api/security/user", `{"username":"ann","password":"secret-1","role":"user"}`, cookie...),
				simple("users", "GET", "/api/security/user", cookie, ""),
				simple("user-get", "GET", "/api/security/user/ann", cookie, ""),
				jreq("user-password", "PUT", "/api/security/user/ann/password", `{"password":"secret-2"}`, cookie...),
				jreq("user-role", "PUT", "/api/security/user/ann/role", `{"role":"admin"}`, cookie...),
				jreq("user-role-bad", "PUT", "/api/security/user/ann/role", `{"role":"king"}`, cookie...),
				jreq("create-as-admin", "POST", "/api/book", `{"id":"u1","title":"Owned"}`, cookie...),
				simple("list-as-admin", "GET", "/api/book", cookie, ""),
				simple("acl-all", "GET", "/api/acl/ann", cookie, ""),
				simple("acl-entity", "GET", "/api/acl/ann/book", cookie, ""),
				jreq("acl-update", "PUT", "/api/acl/ann/book", `{"permissions":{"read":true,"create":false}}`, cookie...),
				simple("acl-default", "PUT", "/api/acl/ann/book/default", cookie, ""),
				simple("config-as-admin", "GET", "/config", cookie, ""),
				simple("kv-as-admin", "PUT", "/kv/k", cookie, "v"),
				simple("basic-header-in-user-mode", "GET", "/api/book", []string{basicAuth("admin", "admin")}, ""),
				simple("user-delete", "DELETE", "/api/security/user/ann", cookie, ""),
				simple("user-delete-unknown", "DELETE", "/api/security/user/ghost", cookie, ""),
				simple("logout", "POST", "/api/security/logout", cookie, ""),
				simple("me-after-logout", "GET", "/api/security/me", cookie, ""),
			}
			return c
		}},
		{name: "basic", config: config("    enabled: true\n    mode: basic\n", ""), cases: func() []tcase {
			auth := []string{basicAuth("admin", "admin")}
			c := []tcase{
				simple("no-credentials", "GET", "/api/book", nil, ""),
				simple("wrong-credentials", "GET", "/api/book", []string{basicAuth("admin", "nope")}, ""),
				simple("malformed-header", "GET", "/api/book", []string{"Authorization: Basic !!!"}, ""),
				simple("bearer-in-basic-mode", "GET", "/api/book", []string{"Authorization: Bearer x"}, ""),
			}
			c = append(c, routeCases(auth)...)
			return c
		}},
		{name: "token", config: config("    enabled: true\n    mode: token\n    token: golden-token\n", "    token: metrics-token\n"), cases: func() []tcase {
			auth := []string{"Authorization: Bearer golden-token"}
			return []tcase{
				simple("no-credentials", "GET", "/api/book", nil, ""),
				simple("wrong-token", "GET", "/api/book", []string{"Authorization: Bearer nope"}, ""),
				simple("lowercase-bearer", "GET", "/api/book", []string{"Authorization: bearer golden-token"}, ""),
				jreq("create", "POST", "/api/book", `{"id":"k1","title":"Token"}`, auth...),
				simple("list", "GET", "/api/book", auth, ""),
				simple("kv", "PUT", "/kv/t", auth, "v"),
				simple("config", "GET", "/config", auth, ""),
				{slug: "metrics-no-token", steps: []step{{data: reqClose("GET", "/metrics", nil, "")}}, read: closeRead, compare: "exact"},
				{slug: "metrics-wrong-token", steps: []step{{data: reqClose("GET", "/metrics", []string{"Authorization: Bearer nope"}, "")}}, read: closeRead, compare: "exact"},
				{slug: "metrics-token", steps: []step{{data: reqClose("GET", "/metrics", []string{"Authorization: Bearer metrics-token"}, "")}}, read: closeRead, compare: "head-only"},
				{slug: "metrics-api-token", steps: []step{{data: reqClose("GET", "/metrics", auth, "")}}, read: closeRead, compare: "head-only"},
			}
		}},
		{name: "strict", config: strings.Replace(config(authOff, ""), "strict: false", "strict: true", 1), cases: func() []tcase {
			return []tcase{
				jreq("create-type", "POST", "/api/art/create", `{"fields":{"title":{"type":"string","required":true}}}`),
				jreq("put-schema", "PUT", "/api/art/schema", `{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"}}}`),
				jreq("create-ok", "POST", "/api/art", `{"id":"a1","title":"T","year":1}`),
				jreq("create-missing", "POST", "/api/art", `{"id":"a2","year":1}`),
				jreq("create-unknown-field", "POST", "/api/art", `{"id":"a3","title":"T","color":"red"}`),
				jreq("create-unknown-entity", "POST", "/api/other", `{"id":"o1"}`),
				jreq("update-bad", "PUT", "/api/art/a1", `{"year":"x"}`),
				jreq("import-rejected", "POST", "/api/import", `{"art":[{"id":"a9"}]}`),
			}
		}},
		{name: "cors", config: configWith(authOff, ", cors: { allowedOrigins: [ \"http://allowed.test\" ] }", "", ""), cases: func() []tcase {
			return []tcase{
				simple("preflight-allowed", "OPTIONS", "/api/book", []string{"Origin: http://allowed.test", "Access-Control-Request-Method: POST", "Access-Control-Request-Headers: content-type"}, ""),
				simple("preflight-denied", "OPTIONS", "/api/book", []string{"Origin: http://denied.test", "Access-Control-Request-Method: POST"}, ""),
				simple("get-allowed-origin", "GET", "/api/book", []string{"Origin: http://allowed.test"}, ""),
				simple("get-denied-origin", "GET", "/api/book", []string{"Origin: http://denied.test"}, ""),
				simple("get-no-origin", "GET", "/api/book", nil, ""),
			}
		}},
		{name: "disk-full", config: config(authOff, ""), before: func() {
			freeSpace.Store(1 << 40)
			storage.FreeSpaceProbe = func() (int64, error) { return freeSpace.Load(), nil }
		}, seed: func(addr string) {
			postJSON(addr, "/api/book", `{"id":"d1","title":"Before"}`)
			freeSpace.Store(0)
		}, after: func() { storage.FreeSpaceProbe = nil }, cases: func() []tcase {
			return []tcase{
				func() tcase {
					c := jreq("write-refused", "POST", "/api/book", `{"id":"d2","title":"Refused"}`)
					c.steps[0].pause = 1500
					return c
				}(),
				simple("kv-refused", "PUT", "/kv/k", nil, "v"),
				simple("delete-allowed", "DELETE", "/api/book/d1", nil, ""),
				simple("read-allowed", "GET", "/api/book", nil, ""),
			}
		}},
		{name: "deadline", config: configWith(authOff, "", "  query:\n    timeoutMs: 1\n", ""), seed: seedBig, cases: func() []tcase {
			return []tcase{
				jreq("query-deadline", "POST", "/api/query", `{"entity":"big","filters":{"text":{"contains":"zzz"}},"sorts":{"text":"desc"}}`),
				simple("list-deadline", "GET", "/api/big?search=*zzz*&sort[text]=desc", nil, ""),
				simple("health", "GET", "/health", nil, ""),
			}
		}},
		{name: "limit", config: config(authOff, ""), cases: func() []tcase {
			return []tcase{
				{slug: "hold-connection", server: "limited", steps: []step{{data: "GET /health HTTP/1.1\r\nHost: h\r\n\r\n"}}, read: quietRead(500), compare: "exact"},
				{slug: "concurrency-503", server: "limited", steps: []step{{data: "GET /health HTTP/1.1\r\nHost: h\r\n\r\n"}}, read: readSpec{Until: "close", MaxMs: 5000}, compare: "d27"},
			}
		}},
		{name: "timeouts", config: config(authOff, ""), cases: func() []tcase {
			return []tcase{
				rawCase("header-read-timeout", "GET /health HTTP/1.1\r\nHost: h\r\n", readSpec{Until: "close", MaxMs: 10000}, "exact"),
				rawCase("body-read-timeout", "POST /api/book HTTP/1.1\r\nHost: h\r\nContent-Type: application/json\r\nContent-Length: 50\r\n\r\n{\"id\":", readSpec{Until: "close", MaxMs: 10000}, "exact"),
				rawCase("idle-close", "GET /health HTTP/1.1\r\nHost: h\r\n\r\n", readSpec{Until: "close", MaxMs: 40000}, "exact"),
				{slug: "silent-connection", steps: nil, read: readSpec{Until: "close", MaxMs: 35000}, compare: "exact"},
			}
		}},
	}
}
