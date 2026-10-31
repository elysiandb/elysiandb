package api_transaction

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
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

	existing := engine.ReadEntityById(entity, id)
	if existing == nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"entity not found"}`)

		return
	}

	if !acl.CanUpdateEntity(security.GetCurrentUsername(ctx), entity, id, payload) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	if globals.GetConfig().Api.Schema.Enabled && entity != schema.SchemaEntity {
		var schemaData map[string]any

		if engine.IsEngineMongoDB() {
			schemaData = mongodb.GetEntitySchema(entity)
		} else {
			schemaData = nil
		}

		errors := schema.ValidateEntityUpdate(entity, existing, payload, schemaData)
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
