package engines

import (
	"net/url"
	"testing"
)

func seedLibrary(s *session) {
	s.t.Helper()

	s.expectStatus(s.post("/api/company", []any{
		map[string]any{"id": "c1", "name": "Acme", "country": "FR"},
		map[string]any{"id": "c2", "name": "Globex", "country": "US"},
	}), 200)

	s.expectStatus(s.post("/api/post", []any{
		map[string]any{"id": "p1", "title": "Go patterns", "views": 120, "published": true, "created": "2024-01-15T10:00:00Z", "author": map[string]any{"@entity": "user", "id": "u1"}, "reviewers": []any{map[string]any{"@entity": "user", "id": "u2"}, map[string]any{"@entity": "user", "id": "u3"}}},
		map[string]any{"id": "p2", "title": "Rust tricks", "views": 80, "published": false, "created": "2024-01-15T12:30:00+02:00", "author": map[string]any{"@entity": "user", "id": "u3"}},
		map[string]any{"id": "p3", "title": "go generics", "views": 300, "published": true, "created": "2023-12-31", "author": map[string]any{"@entity": "user", "id": "u2"}, "tags": []any{"go", 1, true}},
		map[string]any{"id": "p4", "title": "Untitled", "created": "2024-02-01T00:00:00.5Z", "author": map[string]any{"@entity": "user", "id": "ghost"}},
		map[string]any{"id": "p5", "title": "Zig", "views": 80, "published": "yes", "rating": nil},
	}), 200)
	s.expectStatus(s.post("/api/user", []any{
		map[string]any{"id": "u1", "name": "Alice", "age": 30, "tags": []any{"go", "rust"}, "address": map[string]any{"city": "Paris", "zip": "75001"}, "company": map[string]any{"@entity": "company", "id": "c1"}},
		map[string]any{"id": "u2", "name": "bob", "age": 25, "tags": []any{"python", "go"}, "address": map[string]any{"city": "London"}, "company": map[string]any{"@entity": "company", "id": "c2"}},
		map[string]any{"id": "u3", "name": "Charlie", "age": 35, "tags": []any{"rust", "c++"}, "address": map[string]any{"city": "Berlin"}, "company": map[string]any{"@entity": "company", "id": "missing"}},
		map[string]any{"id": "u4", "name": "İlker", "tags": []any{}, "active": true},
	}), 200)

}

func TestFiltersOperatorsAndSearch(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		seedLibrary(s)

		cases := []struct {
			query string
			ids   []string
		}{
			{"filter[name][eq]=Alice", []string{"u1"}},
			{"filter[name][eq]=alice", []string{"u1"}},
			{"filter[name][eq]=BOB", []string{"u2"}},
			{"filter[name][eq]=*li*", []string{"u1", "u3"}},
			{"filter[name][eq]=ilker", []string{"u4"}},
			{"filter[name][neq]=Alice", []string{"u2", "u3", "u4"}},
			{"filter[age][gt]=26", []string{"u1", "u3", "u4"}},
			{"filter[age][gte]=30", []string{"u1", "u3", "u4"}},
			{"filter[age][lt]=30", []string{"u2", "u4"}},
			{"filter[age][lte]=30&filter[age][gt]=25", []string{"u1", "u4"}},
			{"filter[age][eq]=abc", []string{"u4"}},
			{"filter[tags][contains]=go", []string{"u1", "u2"}},
			{"filter[tags][not_contains]=go", []string{"u3", "u4"}},
			{"filter[tags][any]=python,c%2B%2B", []string{"u2", "u3"}},
			{"filter[tags][all]=go,rust", []string{"u1"}},
			{"filter[tags][none]=go,rust", []string{"u4"}},
			{"filter[tags][eq]=go,python", []string{"u2"}},
			{"filter[active][eq]=true", []string{"u1", "u2", "u3", "u4"}},
			{"filter[active][eq]=false", []string{"u1", "u2", "u3"}},
			{"filter[address.city][eq]=paris", []string{"u1", "u4"}},
			{"filter[address.zip][eq]=75001", []string{"u1", "u2", "u3", "u4"}},
			{"filter[company.name][eq]=Acme", []string{"u1", "u3", "u4"}},
			{"filter[company.country][neq]=FR", []string{"u2", "u3", "u4"}},
			{"filter[id][eq]=u2", []string{"u2"}},
			{"filter[missing][eq]=x", []string{"u1", "u2", "u3", "u4"}},
		}

		for _, c := range cases {
			s.expectIds(s.get("/api/user?"+c.query), c.ids...)
		}

		postCases := []struct {
			query string
			ids   []string
		}{
			{"filter[created][gt]=2024-01-15T10:00:00Z", []string{"p2", "p4", "p5"}},
			{"filter[created][gte]=2024-01-15", []string{"p1", "p2", "p4", "p5"}},
			{"filter[created][eq]=2024-01-15", []string{"p1", "p2", "p5"}},
			{"filter[created][lt]=2024-01-01", []string{"p3", "p5"}},
			{"filter[views][gte]=100&filter[published][eq]=true", []string{"p1", "p3", "p4"}},
			{"filter[author.name][eq]=Alice", []string{"p1", "p4", "p5"}},
			{"filter[author.company.country][eq]=US", []string{"p2", "p3", "p4", "p5"}},
			{"filter[tags][contains]=1", []string{"p1", "p2", "p3", "p4", "p5"}},
			{"filter[rating][eq]=x", []string{"p1", "p2", "p3", "p4", "p5"}},
		}

		for _, c := range postCases {
			s.expectIds(s.get("/api/post?"+c.query), c.ids...)
		}

		s.get("/api/post?filter[author.name][eq]=Alice&fields=title,author.name")
		s.get("/api/post?filter[author.name][eq]=Alice&fields=title,author.name,author.company")
		s.get("/api/post?filter[views][gte]=100&limit=1&offset=1")
		s.expectCount(s.get("/api/post?filter[views][gte]=100&countOnly=true"), 3)

		searches := []struct {
			query string
			ids   []string
		}{
			{"search=*go*", []string{"p1", "p3"}},
			{"search=rust*", []string{"p2"}},
			{"search=*80*", []string{}},
			{"search=2024-01-15*", []string{"p1", "p2"}},
			{"search=nothing", []string{}},
			{"search=*&limit=2", []string{"p1", "p2"}},
			{"search=zig&limit=2", []string{}},
			{"search=zig&filter[views][eq]=80", []string{"p5"}},
			{"search=*alice*&includes=author", []string{"p1"}},
			{"search=*globex*&includes=all", []string{"p3"}},
		}

		for _, c := range searches {
			s.expectIds(s.get("/api/post?"+c.query), c.ids...)
		}
	})
}

func TestSortingAndPagination(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		seedLibrary(s)

		cases := []struct {
			query string
			ids   []string
		}{
			{"sort[views]=asc", []string{"p4", "p2", "p5", "p1", "p3"}},
			{"sort[views]=desc", []string{"p3", "p1", "p2", "p5", "p4"}},
			{"sort[created]=asc", []string{"p5", "p3", "p1", "p2", "p4"}},
			{"sort[created]=desc", []string{"p4", "p2", "p1", "p3", "p5"}},
			{"sort[title]=asc", []string{"p1", "p2", "p4", "p5", "p3"}},
			{"sort[published]=asc", []string{"p4", "p5", "p2", "p1", "p3"}},
			{"sort[author.name]=asc", []string{"p4", "p5", "p1", "p2", "p3"}},
			{"sort[author.name]=desc", []string{"p3", "p2", "p1", "p4", "p5"}},
			{"sort[author.company.name]=asc", []string{"p2", "p4", "p5", "p1", "p3"}},
			{"sort[views]=asc&limit=2", []string{"p4", "p2"}},
			{"sort[views]=asc&limit=2&offset=2", []string{"p5", "p1"}},
			{"sort[views]=desc&offset=4", []string{"p4"}},
			{"sort[views]=desc&offset=10", []string{}},
			{"limit=2&offset=1", []string{"p2", "p3"}},
			{"sort[views]=asc&filter[published][eq]=true&limit=2", []string{"p4", "p1"}},
			{"sort[unknown]=asc", []string{"p1", "p2", "p3", "p4", "p5"}},
			{"sort[views]=sideways", []string{"p1", "p2", "p3", "p4", "p5"}},
		}

		for _, c := range cases {
			s.expectIds(s.get("/api/post?"+c.query), c.ids...)
		}

		s.expectIds(s.get("/api/user?sort[tags]=asc"), "u4", "u3", "u1", "u2")
		s.expectIds(s.get("/api/user?sort[tags]=desc"), "u1", "u3", "u2", "u4")
		s.expectIds(s.get("/api/user?sort[age]=desc"), "u3", "u1", "u2", "u4")

		s.put("/api/post/p2", map[string]any{"views": 999})
		s.expectIds(s.get("/api/post?sort[views]=desc&limit=1"), "p2")
		s.delete("/api/post/p2")
		s.expectIds(s.get("/api/post?sort[views]=desc&limit=1"), "p3")
	})
}

func TestIncludesAndFields(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		seedLibrary(s)

		s.expectJSON(s.get("/api/post/p1?includes=author"), `{"id":"p1","title":"Go patterns","views":120,"published":true,"created":"2024-01-15T10:00:00Z",
			"author":{"@entity":"user","id":"u1","name":"Alice","age":30,"tags":["go","rust"],"address":{"city":"Paris","zip":"75001"},"company":{"@entity":"company","id":"c1"}},
			"reviewers":[{"@entity":"user","id":"u2"},{"@entity":"user","id":"u3"}]}`)

		s.expectJSON(s.get("/api/post/p1?includes=author.company,reviewers&fields=author.company.name,reviewers"), `{"author":{"company":{"name":"Acme"}},
			"reviewers":[{"@entity":"user","id":"u2","name":"bob","age":25,"tags":["python","go"],"address":{"city":"London"},"company":{"@entity":"company","id":"c2"}},
			{"@entity":"user","id":"u3","name":"Charlie","age":35,"tags":["rust","c++"],"address":{"city":"Berlin"},"company":{"@entity":"company","id":"missing"}}]}`)

		s.expectJSON(s.get("/api/post/p4?includes=all"), `{"id":"p4","title":"Untitled","created":"2024-02-01T00:00:00.5Z","author":{"@entity":"user","id":"ghost"}}`)
		s.get("/api/post?includes=all")
		s.get("/api/post?includes=author,author.company&fields=id,author.name,author.company.country")
		s.get("/api/post?includes=reviewers.company")
		s.get("/api/user?includes=company&fields=name,company.name,address.city")
		s.expectJSON(s.get("/api/post/p1"), `{"id":"p1","title":"Go patterns","views":120,"published":true,"created":"2024-01-15T10:00:00Z","author":{"@entity":"user","id":"u1"},"reviewers":[{"@entity":"user","id":"u2"},{"@entity":"user","id":"u3"}]}`)

		s.post("/api/node", []any{
			map[string]any{"id": "n1", "next": map[string]any{"@entity": "node", "id": "n2"}, "label": "one"},
			map[string]any{"id": "n2", "next": map[string]any{"@entity": "node", "id": "n1"}, "label": "two"},
		})
		s.expectJSON(s.get("/api/node/n1?includes=all"), `{"id":"n1","label":"one","next":{"@entity":"node","id":"n2","label":"two","next":{"@entity":"node","id":"n1","label":"one","next":{"@entity":"node","id":"n2"}}}}`)
		s.get("/api/node?includes=all&sort[next.label]=asc")
		s.get("/api/node?includes=next.next.next.next")

		s.post("/api/doc", map[string]any{"id": "d1", "meta": map[string]any{"owner": map[string]any{"@entity": "user", "id": "u1"}}, "tags": []any{"a", "b"}})
		s.expectJSON(s.get("/api/doc/d1?includes=meta.owner&fields=meta.owner.name"), `{"meta":{"owner":{"name":"Alice"}}}`)
		s.expectJSON(s.get("/api/doc/d1"), `{"id":"d1","meta":{"owner":{"@entity":"user","id":"u1"}},"tags":["a","b"]}`)
		s.expectJSON(s.get("/api/doc/d1?includes=tags"), `{"id":"d1","meta":{"owner":{"@entity":"user","id":"u1"}},"tags":[]}`)
		s.expectJSON(s.get("/api/doc/d1"), `{"id":"d1","meta":{"owner":{"@entity":"user","id":"u1"}},"tags":["a","b"]}`)
	})
}

func TestQueryAPI(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		seedLibrary(s)

		queries := []struct {
			body map[string]any
			ids  []string
		}{
			{map[string]any{"entity": "user", "filters": map[string]any{"name": map[string]any{"eq": "alice"}}}, []string{"u1"}},
			{map[string]any{"entity": "user", "filters": map[string]any{"or": []any{
				map[string]any{"age": map[string]any{"lt": "28"}},
				map[string]any{"tags": map[string]any{"contains": "rust"}},
			}}, "sorts": map[string]any{"name": "asc"}}, []string{"u1", "u3", "u2"}},
			{map[string]any{"entity": "user", "filters": map[string]any{"and": []any{
				map[string]any{"age": map[string]any{"gte": "25"}},
				map[string]any{"or": []any{
					map[string]any{"address.city": map[string]any{"eq": "Paris"}},
					map[string]any{"address.city": map[string]any{"eq": "berlin"}},
				}},
			}}}, []string{"u1", "u3"}},
			{map[string]any{"entity": "user", "filters": map[string]any{"missing": map[string]any{"eq": "x"}}}, []string{}},
			{map[string]any{"entity": "user", "filters": map[string]any{"tags": map[string]any{"none": "go,rust"}}}, []string{"u4"}},
			{map[string]any{"entity": "user", "filters": map[string]any{"tags": map[string]any{"all": "go,rust"}}}, []string{"u1"}},
			{map[string]any{"entity": "user", "filters": map[string]any{"tags": map[string]any{"any": "c++,python"}}}, []string{"u2", "u3"}},
			{map[string]any{"entity": "user", "filters": map[string]any{"tags": map[string]any{"not_contains": "go"}}}, []string{"u3", "u4"}},
			{map[string]any{"entity": "user", "filters": map[string]any{"name": map[string]any{"neq": "*li*"}}}, []string{"u2", "u4"}},
			{map[string]any{"entity": "user", "filters": map[string]any{"active": map[string]any{"eq": "1"}}}, []string{"u4"}},
			{map[string]any{"entity": "post", "filters": map[string]any{"created": map[string]any{"gte": "2024-01-15", "lt": "2024-02-01"}}, "sorts": map[string]any{"created": "desc"}}, []string{"p2", "p1"}},
			{map[string]any{"entity": "post", "filters": map[string]any{"views": map[string]any{"gt": "50"}}, "sorts": map[string]any{"views": "desc"}, "offset": 1, "limit": 2}, []string{"p1", "p2"}},
			{map[string]any{"entity": "post", "filters": map[string]any{"author.name": map[string]any{"eq": "Alice"}}}, []string{}},
			{map[string]any{"entity": "post", "filters": map[string]any{"author.id": map[string]any{"eq": "u1"}}}, []string{"p1"}},
			{map[string]any{"entity": "post", "filters": map[string]any{"reviewers.id": map[string]any{"eq": "u3"}}}, []string{"p1"}},
			{map[string]any{"entity": "post", "filters": map[string]any{"and": []any{}}}, []string{}},
			{map[string]any{"entity": "post", "offset": -3, "limit": 2}, []string{"p1", "p2"}},
			{map[string]any{"entity": "nothing"}, []string{}},
		}

		for _, q := range queries {
			s.expectIds(s.query(q.body), q.ids...)
		}

		s.expectCount(s.query(map[string]any{"entity": "post", "countOnly": true, "filters": map[string]any{"published": map[string]any{"eq": "true"}}}), 2)
		s.expectJSON(s.query(map[string]any{"entity": "user", "fields": "name,address.city", "filters": map[string]any{"age": map[string]any{"gte": "30"}}, "sorts": map[string]any{"age": "asc"}}),
			`[{"name":"Alice","address":{"city":"Paris"}},{"name":"Charlie","address":{"city":"Berlin"}}]`)
		s.expectStatus(s.query(map[string]any{"entity": "user", "filters": map[string]any{"or": "nope"}}), 400)
		s.expectStatus(s.call("POST", "/api/query", "{"), 400)
	})
}

func TestQueryParamsAreEscaped(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		s.post("/api/odd", []any{
			map[string]any{"id": "o1", "label": "a+b", "path": "x/y?z"},
			map[string]any{"id": "o2", "label": "a b", "path": "100%"},
		})

		s.expectIds(s.get("/api/odd?filter[label][eq]="+url.QueryEscape("a+b")), "o1")
		s.expectIds(s.get("/api/odd?filter[path][eq]="+url.QueryEscape("100%")), "o2")
		s.expectIds(s.get("/api/odd?filter[path][eq]="+url.QueryEscape(`x/y\?z`)), "o1")
	})
}
