package transaction_test

import (
	"encoding/json"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
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
