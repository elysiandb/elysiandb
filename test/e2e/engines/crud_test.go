package engines

import (
	"math"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
)

func TestCRUD(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		s.expectStatus(s.post("/api/books", map[string]any{"id": "b1", "title": "Dune", "pages": 412, "tags": []any{"sf", "classic"}}), 200)
		s.expectStatus(s.post("/api/books", []any{
			map[string]any{"id": "b2", "title": "Hyperion", "pages": 482},
			map[string]any{"id": "b3", "title": "Foundation", "pages": 255, "published": "1951-06-01"},
		}), 200)

		created := s.post("/api/books", map[string]any{"title": "No id yet"})
		s.expectStatus(created, 200)
		generated, _ := field(created.JSON, "id").(string)
		if !uuidPattern.MatchString(generated) {
			s.fail("expected a generated uuid, got %q", generated)
		}
		s.expectStatus(s.delete("/api/books/"+generated), 204)

		s.expectJSON(s.get("/api/books/b1"), `{"id":"b1","title":"Dune","pages":412,"tags":["sf","classic"]}`)
		s.expectStatus(s.get("/api/books/unknown"), 404)

		s.expectJSON(s.put("/api/books/b1", map[string]any{"title": "Dune (1965)"}), `{"id":"b1","title":"Dune (1965)","pages":412,"tags":["sf","classic"]}`)
		s.expectJSON(s.get("/api/books/b1"), `{"id":"b1","title":"Dune (1965)","pages":412,"tags":["sf","classic"]}`)
		s.expectJSON(s.put("/api/books/unknown", map[string]any{"title": "x"}), `null`)

		s.expectIds(s.put("/api/books", []any{
			map[string]any{"id": "b2", "pages": 500},
			map[string]any{"id": "unknown", "pages": 1},
			map[string]any{"pages": 2},
		}), "b2")

		s.expectIds(s.get("/api/books"), "b1", "b2", "b3")
		s.expectCount(s.get("/api/books/count"), 3)
		s.expectCount(s.get("/api/books?countOnly=true"), 3)
		s.expectJSON(s.get("/api/books/b2/exists"), `{"exists":true}`)
		s.expectJSON(s.get("/api/books/nope/exists"), `{"exists":false}`)
		s.expectJSON(s.get("/api/books/b2?fields=title,pages"), `{"title":"Hyperion","pages":500}`)

		s.expectStatus(s.delete("/api/books/b2"), 204)
		s.expectStatus(s.delete("/api/books/b2"), 404)
		s.expectIds(s.get("/api/books"), "b1", "b3")

		s.post("/api/books", map[string]any{"id": "b2", "title": "Hyperion again"})
		s.expectIds(s.get("/api/books"), "b1", "b3", "b2")

		s.expectStatus(s.post("/api/books", "not json"), 400)
		s.expectStatus(s.post("/api/books", map[string]any{}), 400)

		s.callUnordered("GET", "/api/entity/types/name", nil)
		s.expectStatus(s.delete("/api/books"), 204)
		s.expectIds(s.get("/api/books"))
		s.expectCount(s.get("/api/books/count"), 0)
		s.callUnordered("GET", "/api/entity/types/name", nil)
	})
}

func TestSubEntities(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		s.expectStatus(s.post("/api/articles", map[string]any{
			"id":    "art1",
			"title": "Nested",
			"author": map[string]any{
				"@entity": "author",
				"id":      "au1",
				"name":    "Ada",
				"job":     map[string]any{"@entity": "job", "id": "j1", "title": "Engineer"},
			},
			"reviewers": []any{
				map[string]any{"@entity": "author", "id": "au2", "name": "Linus"},
				map[string]any{"@entity": "author", "id": "au3", "name": "Grace"},
			},
			"meta": map[string]any{"source": map[string]any{"@entity": "source", "id": "src1", "url": "https://example.org"}},
		}), 200)

		s.expectJSON(s.get("/api/articles/art1"), `{"id":"art1","title":"Nested","author":{"@entity":"author","id":"au1"},"reviewers":[{"@entity":"author","id":"au2"},{"@entity":"author","id":"au3"}],"meta":{"source":{"@entity":"source","id":"src1"}}}`)
		s.expectJSON(s.get("/api/author/au1"), `{"id":"au1","name":"Ada","job":{"@entity":"job","id":"j1"}}`)
		s.expectJSON(s.get("/api/job/j1"), `{"id":"j1","title":"Engineer"}`)
		s.expectJSON(s.get("/api/source/src1"), `{"id":"src1","url":"https://example.org"}`)
		s.expectIds(s.get("/api/author?sort[id]=asc"), "au1", "au2", "au3")

		s.put("/api/articles/art1", map[string]any{
			"author": map[string]any{"@entity": "author", "id": "au4", "name": "Barbara"},
			"editor": map[string]any{"@entity": "author", "id": "au1"},
		})
		s.expectJSON(s.get("/api/articles/art1?fields=author,editor"), `{"author":{"@entity":"author","id":"au4"},"editor":{"@entity":"author","id":"au1"}}`)
		s.expectJSON(s.get("/api/author/au4"), `{"id":"au4","name":"Barbara"}`)
		s.expectJSON(s.get("/api/author/au1"), `{"id":"au1","name":"Ada","job":{"@entity":"job","id":"j1"}}`)

		s.callUnordered("GET", "/api/entity/types/name", nil)
	})
}

func TestTypesAndTimestampsRoundTrip(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		doc := map[string]any{
			"id":       "e1",
			"at":       "2024-01-15T10:30:00.123+02:00",
			"day":      "2024-01-15",
			"utc":      "2024-01-15T08:30:00Z",
			"nanos":    "2024-01-15T08:30:00.123456789Z",
			"int":      42,
			"float":    1.5,
			"big":      1e21,
			"negZero":  math.Copysign(0, -1),
			"bool":     false,
			"null":     nil,
			"empty":    "",
			"list":     []any{1, "two", true, nil, map[string]any{"k": "v"}, []any{"x"}},
			"emptyArr": []any{},
			"nested":   map[string]any{"_id": "kept", "at": "2024-01-15T10:30:00+02:00", "deep": map[string]any{"x": 1}},
			"unicode":  "İstanbul ✓",
		}
		want := `{"id":"e1","at":"2024-01-15T10:30:00.123+02:00","day":"2024-01-15","utc":"2024-01-15T08:30:00Z","nanos":"2024-01-15T08:30:00.123456789Z","int":42,"float":1.5,"big":1e21,"negZero":-0,"bool":false,"null":null,"empty":"","list":[1,"two",true,null,{"k":"v"},["x"]],"emptyArr":[],"nested":{"_id":"kept","at":"2024-01-15T10:30:00+02:00","deep":{"x":1}},"unicode":"İstanbul ✓"}`

		s.expectJSON(s.post("/api/events", doc), want)
		s.expectJSON(s.get("/api/events/e1"), want)
		s.expectIds(s.get("/api/events?filter[at][gt]=2024-01-15T08:00:00Z"), "e1")
		s.expectIds(s.get("/api/events?filter[at][lt]=2024-01-15T08:30:00Z"))
		s.expectIds(s.get("/api/events?filter[day][eq]=2024-01-15T23:00:00Z"), "e1")
		s.expectIds(s.get("/api/events?filter[nested.at][eq]=2024-01-15T08:30:00Z"), "e1")
	})
}

func TestUserFieldNamedUnderscoreId(t *testing.T) {
	withoutSchema := func(c *configuration.Config) { c.Api.Schema.Enabled = false }

	runOnEngines(t, withoutSchema, func(s *session) {
		s.expectStatus(s.post("/api/legacy", []any{
			map[string]any{"id": "l1", "_id": "507f1f77bcf86cd799439011", "name": "first"},
			map[string]any{"id": "l2", "_id": map[string]any{"oid": "abc"}, "name": "second"},
			map[string]any{"id": "l3", "name": "third"},
		}), 200)

		s.expectJSON(s.get("/api/legacy/l1"), `{"id":"l1","_id":"507f1f77bcf86cd799439011","name":"first"}`)
		s.expectIds(s.get("/api/legacy?filter[_id][eq]=507f*"), "l1", "l2", "l3")
		s.expectIds(s.get("/api/legacy?sort[_id]=desc"), "l2", "l1", "l3")
		s.expectIds(s.query(map[string]any{"entity": "legacy", "filters": map[string]any{"_id": map[string]any{"eq": "507f1f77bcf86cd799439011"}}}), "l1")
		s.put("/api/legacy/l3", map[string]any{"_id": "set later"})
		s.expectJSON(s.get("/api/legacy/l3"), `{"id":"l3","_id":"set later","name":"third"}`)
		s.get("/api/legacy/schema")
	})
}

func TestSortingOnObjectsAndNestedArrays(t *testing.T) {
	withoutSchema := func(c *configuration.Config) { c.Api.Schema.Enabled = false }

	runOnEngines(t, withoutSchema, func(s *session) {
		s.post("/api/shape", []any{
			map[string]any{"id": "s1", "v": map[string]any{"z": 1, "a": 2}, "deep": map[string]any{"list": []any{map[string]any{"n": "zeta"}}}},
			map[string]any{"id": "s2", "v": []any{[]any{1}, map[string]any{"q": 1}, "x"}, "deep": map[string]any{"list": map[string]any{"n": "alpha"}}},
			map[string]any{"id": "s3", "v": "2024-01-01T00:00:00,5Z"},
			map[string]any{"id": "s4", "v": "2024-01-01T00:00:00.0001Z"},
			map[string]any{"id": "s5", "v": "2024-01-01T00:00:00.0002Z"},
			map[string]any{"id": "s6", "v": []any{}},
			map[string]any{"id": "s7", "v": true},
			map[string]any{"id": "s8", "v": map[string]any{"a": 9}},
		})

		s.expectIds(s.get("/api/shape?sort[v]=asc"), "s6", "s2", "s1", "s8", "s7", "s4", "s5", "s3")
		s.expectIds(s.get("/api/shape?sort[v]=desc"), "s3", "s4", "s5", "s7", "s2", "s1", "s8", "s6")
		s.expectIds(s.get("/api/shape?sort[deep.list.n]=asc"), "s1", "s3", "s4", "s5", "s6", "s7", "s8", "s2")
		s.expectIds(s.get("/api/shape?sort[v.0]=desc"), "s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8")
	})
}
