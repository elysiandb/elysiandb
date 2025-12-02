package api_transaction

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/transaction"
	"github.com/valyala/fasthttp"
)

func WriteTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	txID := ctx.UserValue("txId").(string)
	entity := ctx.UserValue("entity").(string)

	var payload map[string]interface{}
	err := json.Unmarshal(ctx.PostBody(), &payload)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid json"}`)

		return
	}

	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		errors := schema.ValidateEntity(entity, payload)
		if len(errors) > 0 {
			ctx.SetStatusCode(fasthttp.StatusBadRequest)
			ctx.SetBodyString(`{"error":"schema validation failed"}`)

			return
		}
	}

	op := transaction.TxOperation{
		Kind:   "write",
		Entity: entity,
		Data:   payload,
	}

	err = transaction.AddOperation(txID, op)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}
