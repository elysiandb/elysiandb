package api

import (
	"encoding/json"
	"strings"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func ListController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	limit := ctx.QueryArgs().GetUintOrZero("limit")
	offset := ctx.QueryArgs().GetUintOrZero("offset")
	sortField, sortAscending := ParseSortParam(ctx.QueryArgs())
	filters := ParseFilterParam(ctx.QueryArgs())
	search := string(ctx.QueryArgs().Peek("search"))
	fieldsParam := string(ctx.QueryArgs().Peek("fields"))
	includesParam := string(ctx.QueryArgs().Peek("includes"))
	countOnlyParam := ctx.QueryArgs().GetBool("countOnly")

	currentUser := ""
	if security.UserAuthenticationIsEnabled() {
		currentUser = security.GetCurrentUsername(ctx)
	}

	var hash []byte
	if !hook.EntityHasHooks(entity) && globals.GetConfig().Api.Cache.Enabled {
		hash = cache.HashQuery(
			entity,
			limit,
			offset,
			sortField,
			sortAscending,
			filters,
			fieldsParam,
			search,
			includesParam,
			countOnlyParam,
			currentUser,
		)
		cached := cache.CacheStore.Get(entity, hash)
		if cached != nil {
			ctx.Response.Header.Set("Content-Type", "application/json")
			ctx.Response.Header.Set("X-Elysian-Cache", "HIT")
			ctx.SetStatusCode(fasthttp.StatusOK)
			ctx.SetBody(cached)

			return
		}
	}

	hooksEnabled := globals.GetConfig().Api.Hooks.Enabled
	preReadHooks := hooksEnabled && hook.EntityHasPreReadHooks(entity)
	postReadHooks := hooksEnabled && hook.EntityHasPostReadHooks(entity)
	paginateAfterHooks := preReadHooks && len(filters) > 0
	hooks := hook.NewRequest(currentUser)

	if engine.IsEngineInternal() && !paginateAfterHooks {
		includes := api_storage.MergeIncludes(includesParam, api_storage.ExtractAutoIncludes(filters))
		fields := api_storage.ParseFieldsParam(fieldsParam)
		mode := acl.ReadModeFor(currentUser, entity)

		if countOnlyParam {
			res := api_storage.Plan(api_storage.ListQuery{
				Entity:    entity,
				Limit:     limit,
				Offset:    offset,
				SortField: sortField,
				SortAsc:   sortAscending,
				Filters:   filters,
				Search:    search,
				Includes:  includes,
				CountOnly: mode != acl.ReadOwned,
			})
			writeCount(ctx, countPlanned(res, limit > 0 || offset > 0 || sortField != "", mode, currentUser), entity, hash)
			return
		}

		if includes == "" && len(fields) == 0 && !preReadHooks && !postReadHooks && mode == acl.ReadAll {
			res := api_storage.Plan(api_storage.ListQuery{
				Entity:    entity,
				Limit:     limit,
				Offset:    offset,
				SortField: sortField,
				SortAsc:   sortAscending,
				Filters:   filters,
				Search:    search,
			})

			if err := writeDocuments(ctx, res.Docs, entity, hash, true); err != nil {
				observe.SetError(ctx, err)
				ctx.SetStatusCode(fasthttp.StatusInternalServerError)
				ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
				ctx.SetBodyString("Error processing request")
			}

			return
		}
	}

	var data []map[string]any
	if paginateAfterHooks && engine.IsEngineInternal() {
		includes := api_storage.MergeIncludes(includesParam, api_storage.ExtractAutoIncludes(filters))
		res := api_storage.Plan(api_storage.ListQuery{Entity: entity, SortField: sortField, SortAsc: sortAscending, Search: search, Includes: includes})
		stored := func(item map[string]any) bool { return api_storage.StoredFiltersMatchEntity(item, filters) }
		matches := func(item map[string]any) bool { return api_storage.FiltersMatchEntity(item, filters) }
		candidates := api_storage.StoredCandidates(res.Snapshot(), filters, nil, includes)
		data = lazyPreReadPage(hooks, entity, res, includes, stored, candidates, acl.ReadModeFor(currentUser, entity), currentUser, matches, offset, limit)
	} else if paginateAfterHooks {
		includes := api_storage.MergeIncludes(includesParam, api_storage.ExtractAutoIncludes(filters))
		data = engine.ListEntities(entity, 0, 0, sortField, sortAscending, nil, search, includes)
		data = api_storage.ApplyStoredFiltersToList(data, filters, 0, 0)
	} else {
		data = engine.ListEntities(entity, limit, offset, sortField, sortAscending, filters, search, includesParam)
	}

	if !paginateAfterHooks || !engine.IsEngineInternal() {
		data = acl.FilterListOfEntities(currentUser, entity, data)
	}

	if paginateAfterHooks && !engine.IsEngineInternal() {
		matches := func(item map[string]any) bool { return api_storage.FiltersMatchEntity(item, filters) }
		data = applyPreReadHooksAndFilter(hooks, entity, data, matches, offset, limit)
	} else if preReadHooks {
		for i, item := range data {
			data[i] = hooks.PreRead(entity, item)
		}

		data = engine.ApplyFiltersToList(data, filters)
	}

	if countOnlyParam {
		writeCount(ctx, len(data), entity, hash)
		return
	}

	if postReadHooks {
		applyPostReadHooks(hooks, entity, data)
	}

	fields := api_storage.ParseFieldsParam(fieldsParam)
	if len(fields) > 0 {
		filteredData := make([]map[string]interface{}, len(data))
		for i, item := range data {
			filteredData[i] = engine.FilterFields(item, fields)
		}

		data = filteredData
	}

	response, err := json.Marshal(data)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
		ctx.SetBodyString("Error processing request")
		return
	}

	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)

	if globals.GetConfig().Api.Cache.Enabled && hash != nil {
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
