package api_transaction

import (
	"github.com/taymour/elysiandb/internal/transaction"
	"github.com/valyala/fasthttp"
)

func RollbackTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	txID := ctx.UserValue("txId").(string)

	err := transaction.RollbackTransaction(txID, transactionOwner(ctx))
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBody([]byte(`{"error":"` + err.Error() + `"}`))

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}
