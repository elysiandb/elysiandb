package http_security

import (
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func GetUsersController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	users, err := security.ListBasicUsers()
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)
		return
	}

	responseBody := `{"users":[`
	for i, user := range users {
		if i > 0 {
			responseBody += ","
		}
		responseBody += `{"username":"` + user["username"].(string) + `","role":"` + user["role"].(string) + `"}`
	}
	responseBody += `]}`

	ctx.SetBodyString(responseBody)
	ctx.SetStatusCode(fasthttp.StatusOK)
}
