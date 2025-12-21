package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/valyala/fasthttp"
)

func ImportController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	var dump map[string][]map[string]interface{}
	if err := json.Unmarshal(ctx.PostBody(), &dump); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBody([]byte(`{"error":"invalid JSON dump"}`))
		return
	}

	engine.ImportAll(dump)

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody([]byte(`{"status":"import completed"}`))
}
