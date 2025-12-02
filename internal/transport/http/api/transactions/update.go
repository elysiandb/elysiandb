package api_transaction

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/transaction"
	"github.com/valyala/fasthttp"
)

func UpdateTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	txID := ctx.UserValue("txId").(string)
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)

	var payload map[string]interface{}
	err := json.Unmarshal(ctx.PostBody(), &payload)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid json"}`)

		return
	}

	existing := api_storage.ReadEntityById(entity, id)
	if existing == nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"entity not found"}`)

		return
	}

	merged := make(map[string]interface{}, len(existing))
	for k, v := range existing {
		merged[k] = v
	}

	for k, v := range payload {
		merged[k] = v
	}

	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		errors := schema.ValidateEntity(entity, merged)
		if len(errors) > 0 {
			ctx.SetStatusCode(fasthttp.StatusBadRequest)
			ctx.SetBodyString(`{"error":"schema validation failed"}`)

			return
		}
	}

	op := transaction.TxOperation{
		Kind:   "update",
		Entity: entity,
		ID:     id,
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
