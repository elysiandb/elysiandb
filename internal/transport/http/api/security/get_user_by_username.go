package http_security

import (
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func GetUserByUsernameController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	username := ctx.UserValue("user_name").(string)

	if canManage, err := security.CurrentUserCanManageUser(ctx, username); err != nil || !canManage {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	user, err := security.GetBasicUserByUsername(username)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)
		return
	}

	responseBody := `{"username":"` + user["username"].(string) + `","role":"` + user["role"].(string) + `"}`

	ctx.SetBodyString(responseBody)
	ctx.SetStatusCode(fasthttp.StatusOK)
}
