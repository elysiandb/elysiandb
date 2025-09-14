package api

import (
	"encoding/json"
	"strings"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func ListController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	limit := ctx.QueryArgs().GetUintOrZero("limit")
	offset := ctx.QueryArgs().GetUintOrZero("offset")
	sortField, sortAscending := ParseSortParam(ctx.QueryArgs())

	data := api_storage.ListEntities(entity, limit, offset, sortField, sortAscending)

	response, err := json.Marshal(data)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString("Error processing request")
		return
	}

	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}

func ParseSortParam(params *fasthttp.Args) (field string, ascending bool) {
	ascending = true

	for k, v := range params.All() {
		key := string(k)
		if strings.HasPrefix(key, "sort[") && strings.HasSuffix(key, "]") {
			field = key[len("sort[") : len(key)-1]
			val := strings.ToLower(strings.TrimSpace(string(v)))
			switch val {
			case "asc":
				ascending = true
				return field, ascending
			case "desc":
				ascending = false
				return field, ascending
			default:
				field = ""
			}
		}
	}

	return field, ascending
}
