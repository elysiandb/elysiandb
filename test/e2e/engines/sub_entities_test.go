package engines

import (
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/security"
)

func coreSchemaWrite() map[string]any {
	return map[string]any{"@entity": "_elysiandb_core_schema", "id": "post", "_manual": true, "fields": map[string]any{}}
}

func TestCoreSubEntitiesCanOnlyBeReferenced(t *testing.T) {
	runOnEngines(t, nil, func(s *session) {
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p0", "title": "seed"}), 200)
		before := canonical(s.get("/api/post/schema").JSON)

		s.expectStatus(s.post("/api/post", map[string]any{"id": "p1", "x": coreSchemaWrite()}), 400)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p2", "hooks": []any{map[string]any{"@entity": "_elysiandb_core_hook", "id": "h1", "name": "x"}}}), 400)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p3", "author": map[string]any{
			"@entity": "author", "id": "a1", "name": "Ada",
			"meta": map[string]any{"acl": map[string]any{"@entity": "_elysiandb_core_acl", "@id": "x", "permissions": map[string]any{}}},
		}}), 400)
		s.expectStatus(s.post("/api/post", []any{
			map[string]any{"id": "p4"},
			map[string]any{"id": "p5", "x": map[string]any{"@entity": "_elysiandb_core_schema", "@id": "post", "_manual": true}},
		}), 400)
		s.expectStatus(s.put("/api/post/p0", map[string]any{"x": coreSchemaWrite()}), 400)
		s.expectStatus(s.put("/api/post", []any{map[string]any{"id": "p0", "x": coreSchemaWrite()}}), 400)
		s.expectStatus(s.post("/api/post/migrate", []any{map[string]any{"set": map[string]any{"x": coreSchemaWrite()}}}), 400)

		tx, _ := field(s.post("/api/tx/begin", nil).JSON, "transaction_id").(string)
		s.expectStatus(s.post("/api/tx/"+tx+"/entity/post", map[string]any{"id": "p6", "x": coreSchemaWrite()}), 400)
		s.expectStatus(s.put("/api/tx/"+tx+"/entity/post/p0", map[string]any{"x": coreSchemaWrite()}), 400)
		s.expectStatus(s.post("/api/tx/"+tx+"/entity/post", map[string]any{"id": "p7", "title": "tx"}), 200)
		s.expectStatus(s.post("/api/tx/"+tx+"/commit", nil), 200)

		s.expectIds(s.get("/api/post"), "p0", "p7")
		s.expectJSON(s.get("/api/post/p0"), `{"id":"p0","title":"seed"}`)
		s.expectStatus(s.get("/api/author/a1"), 404)
		s.expectStatus(s.get("/api/_elysiandb_core_hook/h1"), 404)
		if after := canonical(s.get("/api/post/schema").JSON); after != before {
			s.fail("post schema changed from %s to %s", before, after)
		}

		s.expectStatus(s.post("/api/author", map[string]any{"id": "a0", "name": "Ada"}), 200)
		authorSchema := canonical(s.get("/api/author/schema").JSON)
		authorSchemaLink := map[string]any{"@entity": "_elysiandb_core_schema", "id": "author"}

		s.expectStatus(s.post("/api/post", map[string]any{
			"id":     "p8",
			"schema": authorSchemaLink,
			"hooks":  []any{map[string]any{"@entity": "_elysiandb_core_hook", "@id": "h1"}},
		}), 200)
		s.expectJSON(s.get("/api/post/p8"), `{"id":"p8","schema":{"@entity":"_elysiandb_core_schema","id":"author"},"hooks":[{"@entity":"_elysiandb_core_hook","id":"h1"}]}`)
		s.expectStatus(s.put("/api/post/p0", map[string]any{"schema": authorSchemaLink}), 200)

		s.expectStatus(s.post("/api/post/migrate", []any{map[string]any{"set": map[string]any{"schema._manual": true}}}), 200)
		s.expectStatus(s.post("/api/post/migrate", []any{map[string]any{"set": map[string]any{"archived": true}}}), 200)
		s.expectJSON(s.get("/api/post/p8"), `{"id":"p8","archived":true,"schema":{"@entity":"_elysiandb_core_schema","id":"author"},"hooks":[{"@entity":"_elysiandb_core_hook","id":"h1"}]}`)
		s.expectJSON(s.get("/api/post/p0"), `{"id":"p0","title":"seed","archived":true,"schema":{"@entity":"_elysiandb_core_schema","id":"author"}}`)
		if after := canonical(s.get("/api/author/schema").JSON); after != authorSchema {
			s.fail("author schema changed from %s to %s", authorSchema, after)
		}
	})
}

func TestSubEntityWritesFollowACL(t *testing.T) {
	userMode := func(c *configuration.Config) {
		c.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "user"}
	}

	runOnEngines(t, userMode, func(s *session) {
		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.post("/api/security/user", map[string]any{"username": "alice", "password": "pw", "role": "user"}), 200)
		s.expectStatus(s.post("/api/security/user", map[string]any{"username": "bob", "password": "pw", "role": "user"}), 200)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n0", "text": "admin note"}), 200)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p0", "title": "admin post"}), 200)
		s.expectStatus(s.put("/api/acl/bob/post", map[string]any{"permissions": map[string]any{
			"update": true, "owning_read": true, "owning_write": true, "owning_update": true, "owning_delete": true,
		}}), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p1", "owner": map[string]any{"@entity": "_elysiandb_core_user", "id": "admin", "password": "x", "role": "user"}}), 400)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p1", "acl": map[string]any{"@entity": "_elysiandb_core_acl", "id": "bob::note", "username": "bob", "entity": "note", "permissions": map[string]any{"read": true}}}), 400)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p1", "owner": map[string]any{"@entity": "_elysiandb_core_user", "id": "admin"}}), 200)

		adminNote := map[string]any{"@entity": "note", "id": "n0", "text": "hacked"}
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p2", "note": adminNote}), 403)
		s.expectStatus(s.post("/api/post", []any{map[string]any{"id": "p2"}, map[string]any{"id": "p3", "note": adminNote}}), 403)
		s.expectStatus(s.put("/api/post/p1", map[string]any{"note": adminNote}), 403)
		s.expectStatus(s.put("/api/post", []any{map[string]any{"id": "p1", "note": adminNote}}), 403)
		tx, _ := field(s.post("/api/tx/begin", nil).JSON, "transaction_id").(string)
		s.expectStatus(s.post("/api/tx/"+tx+"/entity/post", map[string]any{"id": "p2", "note": adminNote}), 403)
		s.expectStatus(s.put("/api/tx/"+tx+"/entity/post/p1", map[string]any{"note": adminNote}), 403)
		s.expectStatus(s.post("/api/tx/"+tx+"/commit", nil), 200)
		s.expectIds(s.get("/api/post?sort[id]=asc"), "p1")

		s.expectStatus(s.post("/api/post", map[string]any{"id": "p2", "note": map[string]any{"@entity": "note", "id": "n0"}}), 200)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p3", "note": map[string]any{"@entity": "note", "id": "n1", "text": "bob note", "_elysiandb_core_username": "alice"}}), 200)
		s.expectJSON(s.get("/api/note/n1"), `{"id":"n1","text":"bob note","_elysiandb_core_username":"bob"}`)
		s.expectStatus(s.put("/api/post/p3", map[string]any{"note": map[string]any{"@entity": "note", "id": "n1", "text": "bob edit", "_elysiandb_core_username": "alice"}}), 200)
		s.expectJSON(s.get("/api/note/n1"), `{"id":"n1","text":"bob edit","_elysiandb_core_username":"bob"}`)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p4", "tag": map[string]any{"@entity": "tag", "id": "t1", "label": "new type"}}), 200)
		s.expectJSON(s.get("/api/tag/t1"), `{"id":"t1","label":"new type","_elysiandb_core_username":"bob"}`)

		s.loginAs("alice", security.RoleUser)
		s.expectStatus(s.get("/api/note/n1"), 403)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p5", "note": map[string]any{"@entity": "note", "id": "n1", "text": "alice edit"}}), 403)

		s.loginAs("admin", security.RoleAdmin)
		s.expectJSON(s.get("/api/note"), `[
			{"id":"n0","text":"admin note","_elysiandb_core_username":"admin"},
			{"id":"n1","text":"bob edit","_elysiandb_core_username":"bob"}]`)
		s.expectStatus(s.post("/api/security/login", map[string]any{"username": "admin", "password": security.DefaultAdminPassword}), 200)
		s.expectStatus(s.put("/api/acl/bob/note", map[string]any{"permissions": map[string]any{
			"owning_read": true, "owning_update": true, "owning_delete": true,
		}}), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p6", "note": map[string]any{"@entity": "note", "id": "n2", "text": "no create"}}), 403)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p6", "note": map[string]any{"@entity": "note", "id": "n1", "text": "still mine"}}), 200)

		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.put("/api/acl/bob/note", map[string]any{"permissions": map[string]any{"create": true, "update": true}}), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p7", "note": map[string]any{"@entity": "note", "id": "n0", "text": "granted"}}), 200)
		s.expectStatus(s.post("/api/post", map[string]any{"id": "p8", "note": map[string]any{"@entity": "note", "id": "n2", "text": "created"}}), 200)

		s.loginAs("admin", security.RoleAdmin)
		s.expectJSON(s.get("/api/note"), `[
			{"id":"n0","text":"granted","_elysiandb_core_username":"admin"},
			{"id":"n1","text":"still mine","_elysiandb_core_username":"bob"},
			{"id":"n2","text":"created","_elysiandb_core_username":"bob"}]`)
	})
}
