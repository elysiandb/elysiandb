package api_transaction

import (
	"github.com/taymour/elysiandb/internal/transaction"
	"github.com/valyala/fasthttp"
)

func BeginTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	tx := transaction.BeginTransaction()
	ctx.Response.SetBodyString(`{"transaction_id":"` + tx.ID + `"}`)
	ctx.SetStatusCode(fasthttp.StatusOK)
}
