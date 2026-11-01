package e2e

import (
	"reflect"
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

func TestACLsStillWorkAfterImportingTheACLEntity(t *testing.T) {
	source := startAdminSession(t, nil)
	source.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"alice","password":"alice-pw","role":"admin"}`)
	source.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"id":"b1","title":"Dune"}`)
	dump := source.mustDo(fasthttp.StatusOK, "GET", "/api/export", "").body

	target := startAdminSession(t, nil)
	target.mustDo(fasthttp.StatusOK, "POST", "/api/import", string(dump))

	target.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"bob","password":"bob-pw","role":"admin"}`)
	if acl := target.mustDo(fasthttp.StatusOK, "GET", "/api/acl/bob/book", "").object(t); acl["username"] != "bob" {
		t.Fatalf("expected the new user to get an ACL, got %v", acl)
	}

	target.mustDo(fasthttp.StatusOK, "PUT", "/api/acl/bob/book", `{"permissions":{"read":true,"create":false,"update":false,"delete":false,"owning_read":false,"owning_write":false,"owning_update":false,"owning_delete":false}}`)
	target.mustDo(fasthttp.StatusOK, "POST", "/api/film", `{"id":"f1","title":"Alien"}`)
	target.mustDo(fasthttp.StatusOK, "GET", "/api/film/f1", "")
}
