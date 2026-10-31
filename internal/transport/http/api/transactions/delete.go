package api_transaction

import (
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/transaction"
	"github.com/valyala/fasthttp"
)

func DeleteTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	txID := ctx.UserValue("txId").(string)
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)

	if !acl.CanDeleteEntity(entity, engine.ReadEntityById(entity, id)) {
		sendWriteError(ctx, acl.ErrForbidden)

		return
	}

	op := transaction.TxOperation{
		Kind:   "delete",
		Entity: entity,
		ID:     id,
		Data:   nil,
	}

	err := transaction.AddOperation(txID, transactionOwner(ctx), op)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}
