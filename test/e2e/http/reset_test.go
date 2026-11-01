package e2e

import (
	"testing"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
)

func TestResetDeletesKVKeysAndKeepsTheRESTAPIWorking(t *testing.T) {
	s := startAdminSession(t, func(cfg *configuration.Config) { cfg.Api.Schema.Strict = false })

	s.mustDo(fasthttp.StatusOK, "POST", "/api/books", `{"title":"Dune"}`)
	s.mustDo(fasthttp.StatusNoContent, "PUT", "/kv/note", "hello")

	s.mustDo(fasthttp.StatusOK, "POST", "/reset", "")

	s.mustDo(fasthttp.StatusNotFound, "GET", "/kv/note", "")

	if books := s.mustDo(fasthttp.StatusOK, "GET", "/api/books", "").list(t); len(books) != 1 {
		t.Fatalf("documents must still be listed after a reset, got %v", books)
	}

	if users, _ := s.mustDo(fasthttp.StatusOK, "GET", "/api/security/user", "").object(t)["users"].([]any); len(users) == 0 {
		t.Fatalf("users must still be listed after a reset")
	}

	s.mustDo(fasthttp.StatusOK, "POST", "/api/authors", `{"name":"Herbert"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/authors", `{"name":"Asimov"}`)

	if authors := s.mustDo(fasthttp.StatusOK, "GET", "/api/authors", "").list(t); len(authors) != 2 {
		t.Fatalf("expected 2 authors, got %v", authors)
	}

	s.mustDo(fasthttp.StatusOK, "GET", "/api/acl/admin/authors", "")
}

func TestRepairAtStartupFixesAStoreBrokenByAnEarlierReset(t *testing.T) {
	s := startAdminSession(t, func(cfg *configuration.Config) { cfg.Api.Schema.Strict = false })

	s.mustDo(fasthttp.StatusOK, "POST", "/api/books", `{"title":"Dune"}`)

	storage.DeleteByWildcardKey(globals.ApiKeyPrefix + "*")

	s.mustDo(fasthttp.StatusOK, "POST", "/api/authors", `{"name":"Herbert"}`)
	s.mustDo(fasthttp.StatusForbidden, "POST", "/api/authors", `{"name":"Asimov"}`)

	if repair := api_storage.RepairIndexes(); !repair.Changed() {
		t.Fatalf("the broken indexes should be repaired")
	}
	if err := acl.InitACL(); err != nil {
		t.Fatal(err)
	}

	s.mustDo(fasthttp.StatusOK, "POST", "/api/authors", `{"name":"Asimov"}`)

	if books := s.mustDo(fasthttp.StatusOK, "GET", "/api/books", "").list(t); len(books) != 1 {
		t.Fatalf("books should be listed again, got %v", books)
	}
	if authors := s.mustDo(fasthttp.StatusOK, "GET", "/api/authors", "").list(t); len(authors) != 2 {
		t.Fatalf("expected 2 authors, got %v", authors)
	}
}
