package api_transaction

import (
	"encoding/json"
	"errors"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/valyala/fasthttp"
)

func sendWriteError(ctx *fasthttp.RequestCtx, err error) {
	if errors.Is(err, acl.ErrForbidden) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	response, _ := json.Marshal(map[string]string{"error": err.Error()})
	ctx.SetStatusCode(fasthttp.StatusBadRequest)
	ctx.SetBody(response)
}
