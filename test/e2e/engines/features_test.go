package engines

import (
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/security"
)

func TestSchemaInferenceAndEntityTypes(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		s.post("/api/product", map[string]any{"id": "pr1", "name": "Pen", "price": 1.5, "dims": map[string]any{"w": 1, "h": 10}, "tags": []any{map[string]any{"label": "office"}}})
		s.expectJSON(s.get("/api/product/schema"), `{"id":"product","fields":{
			"name":{"name":"name","type":"string","required":false},
			"price":{"name":"price","type":"number","required":false},
			"dims":{"name":"dims","type":"object","required":false,"fields":{"w":{"name":"w","type":"number","required":false},"h":{"name":"h","type":"number","required":false}}},
			"tags":{"name":"tags","type":"array","required":false,"fields":{"label":{"name":"label","type":"string","required":false}}}}}`)

		s.expectStatus(s.post("/api/product", map[string]any{"id": "pr2", "name": 12}), 400)
		s.expectStatus(s.post("/api/product", map[string]any{"id": "pr3", "name": "Cup", "extra": true}), 200)
		s.expectStatus(s.get("/api/unknown/schema"), 404)

		s.expectStatus(s.post("/api/invoice/create", map[string]any{"fields": map[string]any{
			"number": map[string]any{"type": "string", "required": true},
			"total":  map[string]any{"type": "number"},
		}}), 200)
		s.expectStatus(s.post("/api/invoice/create", map[string]any{"fields": map[string]any{}}), 400)
		s.expectStatus(s.post("/api/draft/create", "{"), 400)
		s.expectStatus(s.post("/api/draft2/create", map[string]any{"nofields": true}), 400)
		s.expectStatus(s.get("/api/draft/schema"), 404)

		s.callUnordered("GET", "/api/entity/types/name", nil)
		s.callUnordered("GET", "/api/entity/types", nil)
		s.expectIds(s.get("/api/invoice"))
		s.expectCount(s.get("/api/invoice/count"), 0)

		s.expectJSON(s.put("/api/product/schema", map[string]any{"fields": map[string]any{
			"name":  map[string]any{"type": "string", "required": true},
			"price": map[string]any{"type": "number"},
		}}), `{"id":"product","_manual":true,"fields":{"name":{"name":"name","type":"string","required":true},"price":{"name":"price","type":"number","required":false}}}`)
		s.expectStatus(s.put("/api/nothing/schema", map[string]any{"fields": map[string]any{}}), 404)
		s.expectStatus(s.put("/api/product/schema", map[string]any{"fields": "nope"}), 400)

		s.expectStatus(s.put("/api/product/pr1", map[string]any{"name": "Pencil"}), 200)
		s.get("/api/product/schema")
	})
}

func TestStrictSchema(t *testing.T) {
	strict := func(c *configuration.Config) { c.Api.Schema.Strict = true }

	runOnEngines(t, strict, func(s *session) {
		s.expectStatus(s.post("/api/article", map[string]any{"id": "a0", "title": "seed"}), 200)
		s.expectStatus(s.put("/api/article/schema", map[string]any{"fields": map[string]any{
			"title":     map[string]any{"type": "string", "required": true},
			"published": map[string]any{"type": "boolean", "required": false},
			"author": map[string]any{"type": "object", "required": true, "fields": map[string]any{
				"name": map[string]any{"type": "string", "required": true},
			}},
		}}), 200)

		s.expectStatus(s.post("/api/article", map[string]any{"id": "a1", "title": "Hello", "author": map[string]any{"name": "Ada"}}), 200)
		s.expectStatus(s.callUnordered("POST", "/api/article", map[string]any{"id": "a2", "published": true}), 400)
		s.expectStatus(s.callUnordered("POST", "/api/article", map[string]any{"id": "a3", "title": "x", "author": map[string]any{"name": "Ada"}, "extra": 1}), 400)
		s.expectStatus(s.callUnordered("POST", "/api/article", map[string]any{"id": "a4", "title": "x", "author": map[string]any{"name": 3}}), 400)
		s.expectStatus(s.callUnordered("POST", "/api/article", []any{map[string]any{"id": "a5", "title": "ok", "author": map[string]any{"name": "B"}}, map[string]any{"id": "a6"}}), 400)
		s.expectIds(s.get("/api/article"), "a0", "a1")

		s.expectStatus(s.callUnordered("PUT", "/api/article/a1", map[string]any{"title": 42}), 400)
		s.expectStatus(s.callUnordered("PUT", "/api/article/a1", map[string]any{"unknown": true}), 400)
		s.expectStatus(s.put("/api/article/a1", map[string]any{"title": "Hello again", "author": map[string]any{"name": "Ada"}}), 200)
		s.expectStatus(s.callUnordered("PUT", "/api/article", []any{map[string]any{"id": "a1", "title": 1}}), 400)
		s.expectJSON(s.get("/api/article/a1"), `{"id":"a1","title":"Hello again","author":{"name":"Ada"}}`)

		s.expectJSON(s.get("/api/article/schema"), `{"id":"article","_manual":true,"fields":{
			"title":{"name":"title","type":"string","required":true},
			"published":{"name":"published","type":"boolean","required":false},
			"author":{"name":"author","type":"object","required":true,"fields":{"name":{"name":"name","type":"string","required":true}}}}}`)
	})
}

func TestTransactions(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		s.post("/api/account", map[string]any{"id": "acc1", "balance": 100})

		tx := field(s.post("/api/tx/begin", nil).JSON, "transaction_id")
		txID, _ := tx.(string)
		if txID == "" {
			s.fail("expected a transaction id")
		}

		s.expectStatus(s.post("/api/tx/"+txID+"/entity/account", map[string]any{"id": "acc2", "balance": 50}), 200)
		s.expectStatus(s.put("/api/tx/"+txID+"/entity/account/acc1", map[string]any{"balance": 70}), 200)
		s.expectStatus(s.put("/api/tx/"+txID+"/entity/account/ghost", map[string]any{"balance": 1}), 400)
		s.expectStatus(s.post("/api/tx/"+txID+"/entity/account", map[string]any{"id": "bad", "balance": "nope"}), 400)
		s.expectIds(s.get("/api/account"), "acc1")
		s.expectStatus(s.post("/api/tx/"+txID+"/commit", nil), 200)
		s.expectJSON(s.get("/api/account?sort[id]=asc"), `[{"id":"acc1","balance":70},{"id":"acc2","balance":50}]`)

		tx2, _ := field(s.post("/api/tx/begin", nil).JSON, "transaction_id").(string)
		s.call("DELETE", "/api/tx/"+tx2+"/entity/account/acc2", nil)
		s.post("/api/tx/"+tx2+"/rollback", nil)
		s.expectStatus(s.post("/api/tx/"+tx2+"/commit", nil), 400)
		s.expectIds(s.get("/api/account?sort[id]=asc"), "acc1", "acc2")

		tx3, _ := field(s.post("/api/tx/begin", nil).JSON, "transaction_id").(string)
		s.call("DELETE", "/api/tx/"+tx3+"/entity/account/acc2", nil)
		s.post("/api/tx/"+tx3+"/entity/ledger", map[string]any{"id": "l1", "account": map[string]any{"@entity": "account", "id": "acc3", "balance": 5}})
		s.post("/api/tx/"+tx3+"/commit", nil)
		s.expectIds(s.get("/api/account?sort[id]=asc"), "acc1", "acc3")
		s.expectJSON(s.get("/api/ledger/l1?includes=account"), `{"id":"l1","account":{"@entity":"account","id":"acc3","balance":5}}`)
	})
}

func TestExportImport(t *testing.T) {
	withoutSchema := func(c *configuration.Config) { c.Api.Schema.Enabled = false }

	runOnEngines(t, withoutSchema, func(s *session) {
		seedLibrary(s)

		exported := s.callUnordered("GET", "/api/export", nil)
		dump, ok := exported.JSON.(map[string]any)
		if !ok {
			s.fail("unexpected export %s", exported.Raw)
		}
		if _, leaked := dump["_elysiandb_core_schema"]; leaked {
			s.fail("export must not contain the schema entity")
		}

		s.delete("/api/post")
		s.delete("/api/user")
		s.delete("/api/company")
		s.expectIds(s.get("/api/post"))

		s.expectStatus(s.post("/api/import", dump), 200)
		s.callUnordered("GET", "/api/export", nil)
		s.expectIds(s.get("/api/post?sort[id]=asc"), "p1", "p2", "p3", "p4", "p5")
		s.expectIds(s.get("/api/user?sort[id]=asc"), "u1", "u2", "u3", "u4")
		s.expectJSON(s.get("/api/user/u2?fields=name,age"), `{"name":"bob","age":25}`)

		s.expectStatus(s.post("/api/import", "{"), 400)
		s.expectStatus(s.post("/api/import", map[string]any{"post": []any{map[string]any{"id": "p9", "title": 9}}}), 200)
		s.expectJSON(s.get("/api/post/p9"), `{"id":"p9","title":9}`)
	})
}

func TestMigrations(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		seedLibrary(s)

		s.expectStatus(s.post("/api/user/migrate", []any{map[string]any{"set": []any{map[string]any{"active": true, "profile.lang": "fr"}}}}), 200)
		s.expectJSON(s.get("/api/user?sort[id]=asc&fields=id,active,profile"), `[
			{"id":"u1","active":true,"profile":{"lang":"fr"}},
			{"id":"u2","active":true,"profile":{"lang":"fr"}},
			{"id":"u3","active":true,"profile":{"lang":"fr"}},
			{"id":"u4","active":true,"profile":{"lang":"fr"}}]`)

		s.expectStatus(s.post("/api/post/migrate", []any{map[string]any{"set": map[string]any{"archived": false}}}), 200)
		s.get("/api/post")
		s.expectJSON(s.get("/api/post/p1"), `{"id":"p1","title":"Go patterns","views":120,"published":true,"created":"2024-01-15T10:00:00Z","archived":false,
			"author":{"@entity":"user","id":"u1"},"reviewers":[{"@entity":"user","id":"u2"},{"@entity":"user","id":"u3"}]}`)
		s.get("/api/user?sort[id]=asc")

		s.post("/api/chain", map[string]any{"id": "x3", "title": "third", "next": map[string]any{"@entity": "chain", "id": "x2"}, "related": []any{map[string]any{"@entity": "chain", "id": "x1"}}})
		s.post("/api/chain", map[string]any{"id": "x1", "title": "first", "related": []any{map[string]any{"@entity": "chain", "id": "x2"}}})
		s.post("/api/chain", map[string]any{"id": "x2", "title": "second", "next": map[string]any{"@entity": "chain", "id": "x1"}})
		s.expectIds(s.get("/api/chain"), "x3", "x1", "x2")
		s.expectStatus(s.post("/api/chain/migrate", []any{map[string]any{"set": map[string]any{"migrated": 1}}}), 200)
		s.expectJSON(s.get("/api/chain?sort[id]=asc&fields=id,title,migrated"), `[
			{"id":"x1","title":"first","migrated":1},
			{"id":"x2","title":"second","migrated":1},
			{"id":"x3","title":"third","migrated":1}]`)
		s.expectJSON(s.get("/api/chain/x3?fields=next,related"), `{"next":{"@entity":"chain","id":"x2"},"related":[{"@entity":"chain","id":"x1"}]}`)

		s.expectStatus(s.post("/api/ghost/migrate", []any{map[string]any{"set": map[string]any{"a": 1}}}), 404)
		s.expectStatus(s.post("/api/user/migrate", []any{map[string]any{"drop": map[string]any{"a": 1}}}), 400)
		s.expectStatus(s.post("/api/user/migrate", "nope"), 400)
	})
}

func TestStatsAndCache(t *testing.T) {
	withCache := func(c *configuration.Config) { c.Api.Cache.Enabled = true }

	runOnEngines(t, withCache, func(s *session) {
		seedLibrary(s)

		stats := s.do("GET", "/stats", nil)
		if field(stats.JSON, "entities_count") != "11" {
			s.fail("unexpected stats %s", stats.Raw)
		}

		first := s.get("/api/post?sort[views]=desc")
		second := s.get("/api/post?sort[views]=desc")
		if first.Header["X-Elysian-Cache"] != "MISS" || second.Header["X-Elysian-Cache"] != "HIT" {
			s.fail("unexpected cache headers %v then %v", first.Header, second.Header)
		}

		s.put("/api/post/p1", map[string]any{"views": 1})
		s.expectIds(s.get("/api/post?sort[views]=desc&limit=2"), "p3", "p2")
		s.get("/api/post/p1")
		s.get("/api/post/p1")
	})
}

func TestUserModeACL(t *testing.T) {
	userMode := func(c *configuration.Config) {
		c.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "user"}
	}

	runOnEngines(t, userMode, func(s *session) {
		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.post("/api/security/user", map[string]any{"username": "alice", "password": "pw", "role": "user"}), 200)
		s.expectStatus(s.post("/api/security/user", map[string]any{"username": "bob", "password": "pw", "role": "user"}), 200)
		s.callUnordered("GET", "/api/security/user", nil)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n0", "text": "admin note"}), 200)

		s.loginAs("alice", security.RoleUser)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n1", "text": "alice note"}), 200)
		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n2", "text": "bob note"}), 200)

		s.expectIds(s.get("/api/note"), "n2")
		s.expectCount(s.get("/api/note/count"), 1)
		s.expectStatus(s.get("/api/note/n1"), 403)
		s.put("/api/note/n1", map[string]any{"text": "hacked"})
		s.expectJSON(s.get("/api/note?fields=id,text"), `[{"id":"n2","text":"bob note"}]`)
		s.expectStatus(s.delete("/api/note/n1"), 403)
		s.expectIds(s.query(map[string]any{"entity": "note"}), "n2")
		s.expectStatus(s.delete("/api/note"), 403)

		s.loginAs("admin", security.RoleAdmin)
		s.expectIds(s.get("/api/note"), "n0", "n1", "n2")
		s.get("/api/acl/bob/note")
		s.callUnordered("GET", "/api/acl/bob", nil)
		s.expectStatus(s.put("/api/acl/bob/note", map[string]any{"permissions": map[string]any{"read": true}}), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectIds(s.get("/api/note"), "n0", "n1", "n2")
		s.expectStatus(s.get("/api/note/n1"), 200)
		s.put("/api/note/n1", map[string]any{"text": "still forbidden"})
		s.expectJSON(s.get("/api/note/n1?fields=text"), `{"text":"alice note"}`)

		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.put("/api/acl/bob/note/default", nil), 200)
		s.loginAs("bob", security.RoleUser)
		s.expectIds(s.get("/api/note"), "n2")

		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.delete("/api/security/user/alice"), 200)
		s.callUnordered("GET", "/api/security/user", nil)
		s.expectStatus(s.delete("/api/note"), 204)
		s.get("/api/acl/bob/note")
	})
}

func TestUserModeIncludesFollowACL(t *testing.T) {
	userModeWithCache := func(c *configuration.Config) {
		c.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "user"}
		c.Api.Cache.Enabled = true
	}

	runOnEngines(t, userModeWithCache, func(s *session) {
		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.post("/api/security/user", map[string]any{"username": "alice", "password": "pw", "role": "user"}), 200)
		s.expectStatus(s.post("/api/security/user", map[string]any{"username": "bob", "password": "pw", "role": "user"}), 200)
		s.expectStatus(s.post("/api/secret", map[string]any{"id": "s0", "value": "admin-secret"}), 200)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p0", "title": "admin post"}), 200)

		s.loginAs("alice", security.RoleUser)
		s.expectStatus(s.post("/api/secret", map[string]any{"id": "s1", "value": "alice-secret"}), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/secret", map[string]any{"id": "s2", "value": "bob-secret"}), 200)
		s.expectStatus(s.post("/api/post", map[string]any{
			"id":     "p1",
			"stolen": map[string]any{"@entity": "secret", "id": "s1"},
			"own":    map[string]any{"@entity": "secret", "id": "s2"},
			"admin":  map[string]any{"@entity": "_elysiandb_core_user", "id": "admin"},
			"acl":    []any{map[string]any{"@entity": "_elysiandb_core_acl", "id": "bob::secret"}},
		}), 200)

		s.loginAs("admin", security.RoleAdmin)
		s.expectJSON(s.get("/api/post/p1?includes=all&fields=stolen.value,own.value,admin,acl"), `{
			"stolen":{"value":"alice-secret"},"own":{"value":"bob-secret"},
			"admin":{"@entity":"_elysiandb_core_user","id":"admin"},
			"acl":[{"@entity":"_elysiandb_core_acl","id":"bob::secret"}]}`)
		s.expectJSON(s.get("/api/post/p1?includes=stolen&fields=stolen.value"), `{"stolen":{"value":"alice-secret"}}`)
		s.expectJSON(s.get("/api/post/p1?includes=stolen"), `{"id":"p1","_elysiandb_core_username":"bob",
			"stolen":{"@entity":"secret","id":"s1","value":"alice-secret","_elysiandb_core_username":"alice"},
			"own":{"@entity":"secret","id":"s2"},"admin":{"@entity":"_elysiandb_core_user","id":"admin"},
			"acl":[{"@entity":"_elysiandb_core_acl","id":"bob::secret"}]}`)

		s.loginAs("bob", security.RoleUser)
		s.expectJSON(s.get("/api/post/p1"), `{"id":"p1","_elysiandb_core_username":"bob",
			"stolen":{"@entity":"secret","id":"s1"},"own":{"@entity":"secret","id":"s2"},
			"admin":{"@entity":"_elysiandb_core_user","id":"admin"},
			"acl":[{"@entity":"_elysiandb_core_acl","id":"bob::secret"}]}`)
		s.expectStatus(s.get("/api/secret/s1"), 403)
		s.expectJSON(s.get("/api/post/p1?includes=all&fields=stolen,own.value,admin,acl"), `{
			"stolen":{"@entity":"secret","id":"s1"},"own":{"value":"bob-secret"},
			"admin":{"@entity":"_elysiandb_core_user","id":"admin"},
			"acl":[{"@entity":"_elysiandb_core_acl","id":"bob::secret"}]}`)
		s.expectJSON(s.get("/api/post/p1?includes=stolen&fields=stolen"), `{"stolen":{"@entity":"secret","id":"s1"}}`)
		s.expectJSON(s.get("/api/post/p1?fields=stolen"), `{"stolen":{"@entity":"secret","id":"s1"}}`)
		s.expectJSON(s.get("/api/post?includes=all&fields=id,stolen,own.value,admin"), `[{"id":"p1",
			"stolen":{"@entity":"secret","id":"s1"},"own":{"value":"bob-secret"},
			"admin":{"@entity":"_elysiandb_core_user","id":"admin"}}]`)

		s.expectJSON(s.get("/api/post?filter[stolen.value]=wrong&fields=id,stolen"), `[{"id":"p1","stolen":{"@entity":"secret","id":"s1"}}]`)
		s.expectIds(s.get("/api/post?filter[stolen.value][neq]=alice-secret"), "p1")
		s.expectIds(s.get("/api/post?filter[admin.role]=nobody"), "p1")
		s.expectIds(s.get("/api/post?filter[own.value]=wrong"))
		s.expectIds(s.get("/api/post?filter[own.value]=bob-secret"), "p1")
		s.expectIds(s.get("/api/post?filter[stolen.id]=s2"))

		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.put("/api/acl/bob/secret", map[string]any{"permissions": map[string]any{"read": true}}), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectJSON(s.get("/api/post/p1?includes=stolen,admin&fields=stolen.value,admin"), `{
			"stolen":{"value":"alice-secret"},"admin":{"@entity":"_elysiandb_core_user","id":"admin"}}`)
		s.expectIds(s.get("/api/post?filter[stolen.value]=wrong"))
		s.expectIds(s.get("/api/post?filter[stolen.value]=alice-secret"), "p1")
	})
}

func TestHooks(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		seedLibrary(s)
		s.loginAs("admin", security.RoleAdmin)

		s.expectStatus(s.post("/api/hook/post", map[string]any{
			"name":     "score",
			"event":    "pre_read",
			"language": "javascript",
			"priority": 10,
			"enabled":  true,
			"script":   "function preRead(ctx) { ctx.entity.score = (ctx.entity.views || 0) * 2; return ctx.entity; }",
		}), 200)
		s.expectStatus(s.post("/api/hook/post", map[string]any{
			"name":     "authors",
			"event":    "post_read",
			"language": "javascript",
			"priority": 5,
			"enabled":  true,
			"script":   "function postRead(ctx) { var u = ctx.query('user', {'name': {'eq': 'Alice'}}); ctx.entity.alices = u.length; return ctx.entity; }",
		}), 200)

		s.callUnordered("GET", "/api/hook/post", nil)
		s.expectIds(s.get("/api/post?filter[score][gte]=240"), "p1", "p2", "p3", "p4", "p5")
		s.expectIds(s.get("/api/post?filter[views][gte]=100"), "p1", "p3", "p4")
		s.expectJSON(s.get("/api/post/p2?fields=id,alices"), `{"id":"p2","alices":1}`)
		s.get("/api/post?sort[views]=asc&limit=3")
		s.query(map[string]any{"entity": "post", "filters": map[string]any{"score": map[string]any{"lt": "200"}}})
	})
}
