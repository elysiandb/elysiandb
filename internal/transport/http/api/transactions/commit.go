package api_transaction

import (
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/transaction"
	"github.com/valyala/fasthttp"
)

func CommitTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	txID := ctx.UserValue("txId").(string)
	tx, _ := transaction.GetTransaction(txID)

	err := transaction.CommitTransaction(txID)

	if tx != nil && globals.GetConfig().Api.Cache.Enabled {
		for _, op := range tx.Ops {
			cache.CacheStore.Purge(op.Entity)
		}
	}

	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}
