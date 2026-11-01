package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

const accountsNotImportedMessage = "accounts and ACLs are not imported, use a backup and restore to move them"

var accountEntities = []string{acl.ACLEntity, security.UserEntity}

func ExportController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)

	dump := engine.DumpAll()
	for _, entity := range accountEntities {
		delete(dump, entity)
	}

	response, err := json.Marshal(dump)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBody([]byte(`{"error":"failed to marshal response"}`))
		return
	}

	ctx.SetBody(response)
}
