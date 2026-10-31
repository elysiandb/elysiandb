package api_transaction

import (
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func transactionOwner(ctx *fasthttp.RequestCtx) string {
	if !security.UserAuthenticationIsEnabled() {
		return ""
	}

	username, _ := ctx.UserValue("username").(string)

	return username
}
