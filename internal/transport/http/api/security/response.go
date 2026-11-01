package http_security

import (
	"encoding/json"

	"github.com/valyala/fasthttp"
)

type userView struct {
	Username string `json:"username"`
	Role     string `json:"role"`
}

func writeJSON(ctx *fasthttp.RequestCtx, status int, body any) {
	data, _ := json.Marshal(body)
	ctx.SetStatusCode(status)
	ctx.SetBody(data)
}

func writeError(ctx *fasthttp.RequestCtx, status int, err error) {
	writeJSON(ctx, status, map[string]string{"error": err.Error()})
}

func userViewFrom(user map[string]any) userView {
	username, _ := user["username"].(string)
	role, _ := user["role"].(string)

	return userView{Username: username, Role: role}
}
