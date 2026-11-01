package api_transaction

import (
	"github.com/taymour/elysiandb/internal/transaction"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func RollbackTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	txID := ctx.UserValue("txId").(string)

	err := transaction.RollbackTransaction(txID)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBody([]byte(`{"error":"` + err.Error() + `"}`))

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}
