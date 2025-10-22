package api

import (
	"encoding/json"
	"strings"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func ListController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	limit := ctx.QueryArgs().GetUintOrZero("limit")
	offset := ctx.QueryArgs().GetUintOrZero("offset")
	sortField, sortAscending := ParseSortParam(ctx.QueryArgs())
	filters := ParseFilterParam(ctx.QueryArgs())
	fieldsParam := string(ctx.QueryArgs().Peek("fields"))
	includesParam := string(ctx.QueryArgs().Peek("includes"))

	var hash []byte
	if globals.GetConfig().Api.Cache.Enabled {
		hash = cache.HashQuery(entity, limit, offset, sortField, sortAscending, filters, fieldsParam, includesParam)
		cached := cache.CacheStore.Get(entity, hash)
		if cached != nil {
			ctx.Response.Header.Set("Content-Type", "application/json")
			ctx.SetStatusCode(fasthttp.StatusOK)
			ctx.SetBody(cached)
			return
		}
	}

	data := api_storage.ListEntities(entity, limit, offset, sortField, sortAscending, filters, includesParam)

	fields := api_storage.ParseFieldsParam(fieldsParam)
	if len(fields) > 0 {
		filteredData := make([]map[string]interface{}, len(data))
		for i, item := range data {
			filteredData[i] = api_storage.FilterFields(item, fields)
		}

		data = filteredData
	}

	response, err := json.Marshal(data)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString("Error processing request")
		return
	}

	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)

	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Set(entity, hash, response)
	}
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

func ParseFilterParam(params *fasthttp.Args) map[string]map[string]string {
	filters := make(map[string]map[string]string)
	for k, v := range params.All() {
		key := string(k)
		if strings.HasPrefix(key, "filter[") && strings.HasSuffix(key, "]") {
			inner := key[len("filter[") : len(key)-1]
			val := strings.TrimSpace(string(v))

			field, op := "", "eq"
			if strings.Contains(inner, "][") {
				parts := strings.SplitN(inner, "][", 2)
				field, op = parts[0], parts[1]
			} else {
				field = inner
			}

			if _, ok := filters[field]; !ok {
				filters[field] = make(map[string]string)
			}

			filters[field][op] = val
		}
	}

	return filters
}
