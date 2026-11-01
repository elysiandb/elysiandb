package e2e

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"github.com/valyala/fasthttp"
)

func TestImportRejectsInvalidDocumentsWithoutWritingAny(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/book/create", `{"fields":{"title":{"type":"string","required":true}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"id":"old","title":"Kept"}`)

	resp := s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/import", `{"book":[{"id":"ok","title":"Dune"},{"id":"ko","year":1965}],"other":[{"id":"o1"}]}`).object(t)
	rejected, _ := resp["rejected"].([]any)
	if len(rejected) != 1 {
		t.Fatalf("expected one rejected document, got %v", resp)
	}

	first, _ := rejected[0].(map[string]any)
	if first["entity"] != "book" || first["id"] != "ko" || first["index"] != float64(1) {
		t.Fatalf("expected the rejected document to be identified, got %v", first)
	}

	if ids := s.mustDo(fasthttp.StatusOK, "GET", "/api/book", "").ids(t); !reflect.DeepEqual(ids, []string{"old"}) {
		t.Fatalf("a rejected import must not change the entity, got %v", ids)
	}

	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/book/ok", "")
	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/other/o1", "")
}

func TestImportReplacesTheDocumentsOfAnEntity(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/full", `{"id":"f1","n":1}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/full", `{"id":"f2","n":2}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/untouched", `{"id":"u1"}`)
	s.mustDo(fasthttp.StatusOK, "GET", "/api/full/f2", "")
	s.mustDo(fasthttp.StatusOK, "GET", "/api/full", "")

	s.mustDo(fasthttp.StatusOK, "POST", "/api/import", `{"full":[{"id":"f1","n":10}]}`)

	if ids := s.mustDo(fasthttp.StatusOK, "GET", "/api/full", "").ids(t); !reflect.DeepEqual(ids, []string{"f1"}) {
		t.Fatalf("expected only the imported documents to be listed, got %v", ids)
	}

	if count := s.mustDo(fasthttp.StatusOK, "GET", "/api/full/count", "").object(t)["count"]; count != float64(1) {
		t.Fatalf("expected a count of 1, got %v", count)
	}

	s.mustDo(fasthttp.StatusNotFound, "GET", "/api/full/f2", "")
	if got := s.mustDo(fasthttp.StatusOK, "GET", "/api/full/f1", "").object(t)["n"]; got != float64(10) {
		t.Fatalf("expected the imported version of f1, got %v", got)
	}

	s.mustDo(fasthttp.StatusOK, "GET", "/api/untouched/u1", "")
}

func TestExportLeavesAccountsAndACLsOut(t *testing.T) {
	s := startAdminSession(t, nil)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"alice","password":"alice-pw","role":"admin"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"id":"b1","title":"Dune"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/book", `{"name":"tag","event":"post_read","language":"javascript","enabled":true,"script":"function postRead(ctx) {}"}`)

	dump := s.mustDo(fasthttp.StatusOK, "GET", "/api/export", "")
	if strings.Contains(string(dump.body), "$2a$") {
		t.Fatalf("the export must not contain password hashes: %s", dump.body)
	}

	entities := dump.object(t)
	for _, entity := range []string{"_elysiandb_core_user", "_elysiandb_core_acl"} {
		if _, ok := entities[entity]; ok {
			t.Fatalf("the export must not contain %s", entity)
		}
	}

	for _, entity := range []string{"book", "_elysiandb_core_hook"} {
		if _, ok := entities[entity]; !ok {
			t.Fatalf("expected %s in the export, got %v", entity, entities)
		}
	}
}

func TestImportNeverTouchesAccountsAndACLs(t *testing.T) {
	source := startAdminSession(t, nil)
	source.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"id":"b1","title":"Dune"}`)

	var dump map[string]any
	if err := json.Unmarshal(source.mustDo(fasthttp.StatusOK, "GET", "/api/export", "").body, &dump); err != nil {
		t.Fatalf("invalid export: %v", err)
	}

	dump["_elysiandb_core_user"] = []any{
		map[string]any{"id": "admin", "username": "admin", "password": "$2a$10$fromanotherinstance", "role": "admin"},
		map[string]any{"id": "alice", "username": "alice", "password": "$2a$10$fromanotherinstance", "role": "admin"},
	}
	dump["_elysiandb_core_acl"] = []any{
		map[string]any{"id": "alice::book", "username": "alice", "entity": "book", "permissions": map[string]any{"read": true}},
	}
	body, _ := json.Marshal(dump)

	target := startAdminSession(t, nil)
	target.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"bob","password":"bob-pw","role":"admin"}`)
	target.mustDo(fasthttp.StatusOK, "POST", "/api/movie", `{"id":"m1"}`)
	target.mustDo(fasthttp.StatusOK, "PUT", "/api/acl/bob/movie", `{"permissions":{"read":true,"create":false,"update":false,"delete":false,"owning_read":false,"owning_write":false,"owning_update":false,"owning_delete":false}}`)
	aclsBefore := string(target.mustDo(fasthttp.StatusOK, "GET", "/api/acl/bob/movie", "").body)

	resp := target.mustDo(fasthttp.StatusOK, "POST", "/api/import", string(body)).object(t)
	if !reflect.DeepEqual(resp["ignored"], []any{"_elysiandb_core_acl", "_elysiandb_core_user"}) || !strings.Contains(resp["message"].(string), "backup and restore") {
		t.Fatalf("expected the import to report the ignored entities, got %v", resp)
	}

	anonymous := target.anonymous()
	for _, login := range []struct {
		username, password string
		status             int
	}{
		{"bob", "bob-pw", fasthttp.StatusOK},
		{"admin", "admin", fasthttp.StatusOK},
		{"alice", "alice-pw", fasthttp.StatusUnauthorized},
	} {
		credentials, _ := json.Marshal(map[string]string{"username": login.username, "password": login.password})
		anonymous.mustDo(login.status, "POST", "/api/security/login", string(credentials))
	}

	if got := string(target.mustDo(fasthttp.StatusOK, "GET", "/api/acl/bob/movie", "").body); got != aclsBefore {
		t.Fatalf("the import must not change existing ACLs:\nbefore %s\nafter  %s", aclsBefore, got)
	}

	target.mustDo(fasthttp.StatusOK, "GET", "/api/book/b1", "")
	if acl := target.mustDo(fasthttp.StatusOK, "GET", "/api/acl/bob/book", "").object(t); acl["username"] != "bob" {
		t.Fatalf("expected an ACL for the imported entity type, got %v", acl)
	}

	target.mustDo(fasthttp.StatusNotFound, "GET", "/api/acl/alice/book", "")

	target.mustDo(fasthttp.StatusOK, "POST", "/api/strict/create", `{"fields":{"title":{"type":"string","required":true}}}`)
	rejected := target.mustDo(fasthttp.StatusBadRequest, "POST", "/api/import", `{"_elysiandb_core_user":[],"strict":[{"id":"s1"}]}`).object(t)
	if !reflect.DeepEqual(rejected["ignored"], []any{"_elysiandb_core_user"}) || rejected["rejected"] == nil {
		t.Fatalf("expected a rejected import to report the ignored entities too, got %v", rejected)
	}
}
