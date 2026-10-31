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
	"github.com/taymour/elysiandb/internal/transport/http/api"
	"github.com/valyala/fasthttp"
)

func WriteTransactionController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	txID := ctx.UserValue("txId").(string)
	entity := ctx.UserValue("entity").(string)

	var payload map[string]interface{}
	err := json.Unmarshal(ctx.PostBody(), &payload)
	if err != nil || payload == nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid json"}`)

		return
	}

	if api.RejectCoreEntityReferences(ctx, payload) {
		return
	}

	username := security.GetCurrentUsername(ctx)
	if !acl.CanWriteEntity(username, entity, payload) {
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

		errors := schema.ValidateEntity(entity, payload, schemaData)
		if len(errors) > 0 {
			ctx.SetStatusCode(fasthttp.StatusBadRequest)
			ctx.SetBodyString(`{"error":"schema validation failed"}`)

			return
		}
	}

	if security.UserAuthenticationIsEnabled() {
		payload[acl.UsernameField] = username
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
