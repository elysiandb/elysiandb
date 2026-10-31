package engines

import (
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/security"
)

func userMode(c *configuration.Config) {
	c.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "user"}
}

func seedUsersAndNote(s *session) {
	s.loginAs("admin", security.RoleAdmin)
	s.expectStatus(s.post("/api/security/user", map[string]any{"username": "alice", "password": "pw", "role": "user"}), 200)
	s.expectStatus(s.post("/api/security/user", map[string]any{"username": "bob", "password": "pw", "role": "user"}), 200)
	s.expectStatus(s.post("/api/note", map[string]any{"id": "n0", "text": "admin note"}), 200)
}

func expectAdminIntact(s *session) {
	s.loginAs("admin", security.RoleAdmin)
	s.expectStatus(s.post("/api/security/login", map[string]any{"username": "admin", "password": security.DefaultAdminPassword}), 200)
	s.expectJSON(s.get("/api/security/user/admin"), `{"username":"admin","role":"admin"}`)
}

func TestUserModeDirectWritesFollowACL(t *testing.T) {
	runOnEngines(t, userMode, func(s *session) {
		seedUsersAndNote(s)

		s.loginAs("bob", security.RoleUser)
		takeover := map[string]any{"id": "admin", "username": "admin", "password": "x", "role": "user"}
		s.expectStatus(s.post("/api/_elysiandb_core_user", takeover), 403)
		s.expectStatus(s.post("/api/_elysiandb_core_user", []any{map[string]any{"id": "eve", "username": "eve", "password": "x", "role": "admin"}}), 403)
		s.expectStatus(s.post("/api/_elysiandb_core_acl", map[string]any{"id": "bob::note", "username": "bob", "entity": "note", "permissions": map[string]any{"read": true}}), 403)
		s.expectStatus(s.post("/api/_elysiandb_core_hook", map[string]any{"id": "h1", "entity": "note", "event": "pre_read", "script": "x"}), 403)
		s.expectStatus(s.put("/api/_elysiandb_core_user/admin", map[string]any{"role": "user"}), 403)
		s.expectStatus(s.put("/api/_elysiandb_core_user", []any{takeover}), 403)
		s.expectStatus(s.delete("/api/_elysiandb_core_user/admin"), 403)

		s.expectStatus(s.post("/api/note", map[string]any{"id": "n1", "text": "bob note", "_elysiandb_core_username": "alice"}), 200)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n0", "text": "overwritten"}), 403)
		s.expectStatus(s.post("/api/note", []any{map[string]any{"id": "n2", "text": "first"}, map[string]any{"id": "n0", "text": "overwritten"}}), 403)
		s.expectStatus(s.put("/api/note/n0", map[string]any{"text": "hacked", "_elysiandb_core_username": "bob"}), 403)
		s.expectStatus(s.put("/api/note", []any{map[string]any{"id": "n0", "text": "hacked", "_elysiandb_core_username": "bob"}}), 403)
		s.expectStatus(s.put("/api/note", []any{map[string]any{"id": "n1", "text": "batch"}, map[string]any{"id": "n0", "text": "hacked"}}), 403)
		s.expectJSON(s.put("/api/note/n1", map[string]any{"id": "n0", "text": "bob edit", "_elysiandb_core_username": "alice"}), `{"id":"n1","text":"bob edit","_elysiandb_core_username":"bob"}`)
		s.expectStatus(s.post("/api/tag", map[string]any{"id": "t1", "label": "new type"}), 200)
		s.expectJSON(s.get("/api/tag/t1"), `{"id":"t1","label":"new type","_elysiandb_core_username":"bob"}`)

		expectAdminIntact(s)
		s.expectJSON(s.get("/api/note"), `[
			{"id":"n0","text":"admin note","_elysiandb_core_username":"admin"},
			{"id":"n1","text":"bob edit","_elysiandb_core_username":"bob"}]`)
		s.expectStatus(s.put("/api/acl/bob/note", map[string]any{"permissions": map[string]any{"owning_read": true, "owning_update": true}}), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n3", "text": "no create"}), 403)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n1", "text": "replaced"}), 200)

		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.put("/api/acl/bob/note", map[string]any{"permissions": map[string]any{"create": true, "update": true}}), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n0", "text": "granted"}), 200)
		s.expectStatus(s.put("/api/note", []any{map[string]any{"id": "n1", "text": "batch", "_elysiandb_core_username": "alice"}}), 200)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n3", "text": "created"}), 200)

		s.loginAs("admin", security.RoleAdmin)
		s.expectJSON(s.get("/api/note"), `[
			{"id":"n0","text":"granted","_elysiandb_core_username":"admin"},
			{"id":"n1","text":"batch","_elysiandb_core_username":"bob"},
			{"id":"n3","text":"created","_elysiandb_core_username":"bob"}]`)
	})
}

func TestUserModeTransactionsFollowACL(t *testing.T) {
	runOnEngines(t, userMode, func(s *session) {
		seedUsersAndNote(s)

		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n1", "text": "bob note"}), 200)
		tx, _ := field(s.post("/api/tx/begin", nil).JSON, "transaction_id").(string)
		ops := "/api/tx/" + tx + "/entity/"

		s.expectStatus(s.post(ops+"_elysiandb_core_user", map[string]any{"id": "admin", "username": "admin", "password": "x", "role": "user"}), 403)
		s.expectStatus(s.put(ops+"_elysiandb_core_user/admin", map[string]any{"role": "user"}), 403)
		s.expectStatus(s.delete(ops+"_elysiandb_core_user/admin"), 403)
		s.expectStatus(s.post(ops+"note", map[string]any{"id": "n0", "text": "overwritten"}), 403)
		s.expectStatus(s.put(ops+"note/n0", map[string]any{"text": "hacked", "_elysiandb_core_username": "bob"}), 403)
		s.expectStatus(s.delete(ops+"note/n0"), 403)

		s.expectStatus(s.post(ops+"note", map[string]any{"id": "n2", "text": "tx note", "_elysiandb_core_username": "alice"}), 200)
		s.expectStatus(s.put(ops+"note/n1", map[string]any{"id": "n0", "text": "tx edit", "_elysiandb_core_username": "alice"}), 200)

		s.loginAs("alice", security.RoleUser)
		s.expectStatus(s.post("/api/note", map[string]any{"id": "n3", "text": "alice note"}), 200)
		s.expectStatus(s.post(ops+"note", map[string]any{"id": "n4", "text": "alice tx note"}), 400)
		s.expectStatus(s.delete(ops+"note/n3"), 400)
		s.expectStatus(s.post("/api/tx/"+tx+"/commit", nil), 400)
		s.expectStatus(s.post("/api/tx/"+tx+"/rollback", nil), 200)

		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/tx/"+tx+"/commit", nil), 200)
		tx2, _ := field(s.post("/api/tx/begin", nil).JSON, "transaction_id").(string)
		s.expectStatus(s.delete("/api/tx/"+tx2+"/entity/note/n2"), 200)
		s.expectStatus(s.post("/api/tx/"+tx2+"/commit", nil), 200)

		expectAdminIntact(s)
		s.expectJSON(s.get("/api/note"), `[
			{"id":"n0","text":"admin note","_elysiandb_core_username":"admin"},
			{"id":"n1","text":"tx edit","_elysiandb_core_username":"bob"},
			{"id":"n3","text":"alice note","_elysiandb_core_username":"alice"}]`)

		tx3, _ := field(s.post("/api/tx/begin", nil).JSON, "transaction_id").(string)
		s.expectStatus(s.put("/api/tx/"+tx3+"/entity/note/n1", map[string]any{"text": "admin edit"}), 200)
		s.loginAs("bob", security.RoleUser)
		s.expectStatus(s.post("/api/tx/"+tx3+"/commit", nil), 400)
		s.loginAs("admin", security.RoleAdmin)
		s.expectStatus(s.post("/api/tx/"+tx3+"/commit", nil), 200)
		s.expectJSON(s.get("/api/note/n1"), `{"id":"n1","text":"admin edit","_elysiandb_core_username":"bob"}`)
	})
}
