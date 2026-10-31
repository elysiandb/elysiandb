package transaction_test

import (
	"encoding/json"
	"testing"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	api_transaction "github.com/taymour/elysiandb/internal/transport/http/api/transactions"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Api.Cache.Enabled = false
	cfg.Api.Schema.Enabled = false
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()

	cache.InitCache(30)
	api_storage.DeleteAll()
}

func newCtx(method, uri, body string) *fasthttp.RequestCtx {
	req := fasthttp.AcquireRequest()
	req.SetRequestURI(uri)
	req.Header.SetMethod(method)
	if body != "" {
		req.SetBodyString(body)
	}
	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	return ctx
}

func TestTransactionLifecycle(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/tx/begin", "")
	api_transaction.BeginTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("begin failed")
	}

	var beginResp map[string]string
	json.Unmarshal(ctx.Response.Body(), &beginResp)
	txID := beginResp["transaction_id"]

	ctx = newCtx("POST", "/api/tx/write/books", `{"title":"Dune"}`)
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "books")
	api_transaction.WriteTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("write failed")
	}

	ctx = newCtx("POST", "/api/tx/commit", "")
	ctx.SetUserValue("txId", txID)
	api_transaction.CommitTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("commit failed")
	}
}

func TestTransactionRollback(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/tx/begin", "")
	api_transaction.BeginTransactionController(ctx)

	var beginResp map[string]string
	json.Unmarshal(ctx.Response.Body(), &beginResp)
	txID := beginResp["transaction_id"]

	ctx = newCtx("POST", "/api/tx/write/test", `{"a":1}`)
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "test")
	api_transaction.WriteTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("write failed")
	}

	ctx = newCtx("POST", "/api/tx/rollback", "")
	ctx.SetUserValue("txId", txID)
	api_transaction.RollbackTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("rollback failed")
	}
}

func TestTransactionDelete(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/tx/begin", "")
	api_transaction.BeginTransactionController(ctx)

	var beginResp map[string]string
	json.Unmarshal(ctx.Response.Body(), &beginResp)
	txID := beginResp["transaction_id"]

	ctx = newCtx("POST", "/api/tx/write/test", `{"id":"1","a":1}`)
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "test")
	api_transaction.WriteTransactionController(ctx)

	ctx = newCtx("POST", "/api/tx/commit", "")
	ctx.SetUserValue("txId", txID)
	api_transaction.CommitTransactionController(ctx)

	ctx = newCtx("POST", "/api/tx/begin", "")
	api_transaction.BeginTransactionController(ctx)

	json.Unmarshal(ctx.Response.Body(), &beginResp)
	txID = beginResp["transaction_id"]

	ctx = newCtx("DELETE", "/api/tx/delete/test/1", "")
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "test")
	ctx.SetUserValue("id", "1")
	api_transaction.DeleteTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("delete failed")
	}

	ctx = newCtx("POST", "/api/tx/commit", "")
	ctx.SetUserValue("txId", txID)
	api_transaction.CommitTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("commit after delete failed")
	}
}

func TestTransactionUpdate(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/api/tx/begin", "")
	api_transaction.BeginTransactionController(ctx)

	var beginResp map[string]string
	json.Unmarshal(ctx.Response.Body(), &beginResp)
	txID := beginResp["transaction_id"]

	ctx = newCtx("POST", "/api/tx/write/u", `{"id":"1","a":1}`)
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "u")
	api_transaction.WriteTransactionController(ctx)

	ctx = newCtx("POST", "/api/tx/commit", "")
	ctx.SetUserValue("txId", txID)
	api_transaction.CommitTransactionController(ctx)

	ctx = newCtx("POST", "/api/tx/begin", "")
	api_transaction.BeginTransactionController(ctx)
	json.Unmarshal(ctx.Response.Body(), &beginResp)
	txID = beginResp["transaction_id"]

	ctx = newCtx("PUT", "/api/tx/update/u/1", `{"a":2}`)
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "u")
	ctx.SetUserValue("id", "1")
	api_transaction.UpdateTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("update failed")
	}

	ctx = newCtx("POST", "/api/tx/commit", "")
	ctx.SetUserValue("txId", txID)
	api_transaction.CommitTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("commit after update failed")
	}
}

func setupOwnership(t *testing.T) {
	setup(t)

	cfg := globals.GetConfig()
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
	t.Cleanup(func() { security.SetCurrentUsername("") })

	for _, username := range []string{"alice", "bob"} {
		perms := acl.NewPermissions()
		perms[acl.PermissionOwningUpdate] = true
		perms[acl.PermissionOwningDelete] = true
		a := acl.ACL{Username: username, Entity: "notes", Permissions: perms}
		api_storage.WriteEntity(acl.ACLEntity, a.ToDataMap())
	}

	api_storage.WriteEntity("notes", map[string]any{
		"id":              "1",
		"title":           "alice",
		acl.UsernameField: "alice",
	})
}

func beginAs(username string) string {
	security.SetCurrentUsername(username)

	ctx := newCtx("POST", "/api/tx/begin", "")
	api_transaction.BeginTransactionController(ctx)

	var beginResp map[string]string
	json.Unmarshal(ctx.Response.Body(), &beginResp)

	return beginResp["transaction_id"]
}

func commit(t *testing.T, txID string) {
	ctx := newCtx("POST", "/api/tx/commit", "")
	ctx.SetUserValue("txId", txID)
	api_transaction.CommitTransactionController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("commit failed: %d", ctx.Response.StatusCode())
	}
}

func txUpdateNote(txID, id, body string) *fasthttp.RequestCtx {
	ctx := newCtx("PUT", "/api/tx/"+txID+"/entity/notes/"+id, body)
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "notes")
	ctx.SetUserValue("id", id)
	api_transaction.UpdateTransactionController(ctx)

	return ctx
}

func txDeleteNote(txID, id string) *fasthttp.RequestCtx {
	ctx := newCtx("DELETE", "/api/tx/"+txID+"/entity/notes/"+id, "")
	ctx.SetUserValue("txId", txID)
	ctx.SetUserValue("entity", "notes")
	ctx.SetUserValue("id", id)
	api_transaction.DeleteTransactionController(ctx)

	return ctx
}

func TestTransactionUpdate_OwningUpdateRejectsOtherUsersEntity(t *testing.T) {
	setupOwnership(t)

	txID := beginAs("bob")

	ctx := txUpdateNote(txID, "1", `{"title":"bob","`+acl.UsernameField+`":"bob"}`)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	commit(t, txID)

	stored := api_storage.ReadEntityById("notes", "1")
	if stored["title"] != "alice" || stored[acl.UsernameField] != "alice" {
		t.Fatalf("note was modified: %v", stored)
	}
}

func TestTransactionUpdate_OwnerAllowed(t *testing.T) {
	setupOwnership(t)

	txID := beginAs("alice")

	ctx := txUpdateNote(txID, "1", `{"title":"v2"}`)
	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	commit(t, txID)

	stored := api_storage.ReadEntityById("notes", "1")
	if stored["title"] != "v2" || stored[acl.UsernameField] != "alice" {
		t.Fatalf("unexpected note: %v", stored)
	}
}

func TestTransactionDelete_OwningDeleteRejectsOtherUsersEntity(t *testing.T) {
	setupOwnership(t)

	txID := beginAs("bob")

	ctx := txDeleteNote(txID, "1")
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403, got %d", ctx.Response.StatusCode())
	}

	commit(t, txID)

	if api_storage.ReadEntityById("notes", "1") == nil {
		t.Fatal("note was deleted")
	}
}

func TestTransactionDelete_OwnerAllowed(t *testing.T) {
	setupOwnership(t)

	txID := beginAs("alice")

	ctx := txDeleteNote(txID, "1")
	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	commit(t, txID)

	if api_storage.ReadEntityById("notes", "1") != nil {
		t.Fatal("note was not deleted")
	}
}
