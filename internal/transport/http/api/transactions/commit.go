package api_transaction

import (
	"github.com/taymour/elysiandb/internal/transaction"
	"github.com/valyala/fasthttp"
)

func CommitTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	txID := ctx.UserValue("txId").(string)

	err := transaction.CommitTransaction(txID)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}
