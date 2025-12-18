package api

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"sort"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

type QueryPayload struct {
	Entity    string            `json:"entity"`
	Offset    int               `json:"offset"`
	Limit     int               `json:"limit"`
	Filters   map[string]any    `json:"filters"`
	Sorts     map[string]string `json:"sorts"`
	CountOnly bool              `json:"countOnly"`
	Fields    string            `json:"fields"`
}

func (q *QueryPayload) Hash() []byte {
	normalized := map[string]any{
		"entity":  q.Entity,
		"offset":  q.Offset,
		"limit":   q.Limit,
		"filters": normalizeAny(q.Filters),
		"sorts":   normalizeStringMap(q.Sorts),
	}

	b, _ := json.Marshal(normalized)
	sum := sha256.Sum256(b)

	return sum[:]
}

func normalizeAny(v any) any {
	switch t := v.(type) {
	case map[string]any:
		keys := make([]string, 0, len(t))
		for k := range t {
			keys = append(keys, k)
		}

		sort.Strings(keys)

		out := make(map[string]any, len(t))
		for _, k := range keys {
			out[k] = normalizeAny(t[k])
		}

		return out
	case []any:
		out := make([]any, len(t))
		for i, v := range t {
			out[i] = normalizeAny(v)
		}

		return out
	default:
		return t
	}
}

func normalizeStringMap(m map[string]string) map[string]string {
	if m == nil {
		return nil
	}

	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	out := make(map[string]string, len(m))
	for _, k := range keys {
		out[k] = m[k]
	}
	return out
}

func QueryController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	var payload QueryPayload
	if err := json.Unmarshal(ctx.PostBody(), &payload); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		return
	}

	if payload.Filters == nil {
		payload.Filters = map[string]any{}
	}
	if payload.Sorts == nil {
		payload.Sorts = map[string]string{}
	}

	currentUser := ""
	if security.UserAuthenticationIsEnabled() {
		currentUser = security.GetCurrentUsername()
	}

	var hash []byte
	if !hook.EntityHasHooks(payload.Entity) && globals.GetConfig().Api.Cache.Enabled {
		h := sha256.New()
		h.Write([]byte(payload.Entity))
		h.Write(payload.Hash())
		h.Write([]byte(currentUser))
		hash = h.Sum(nil)

		cached := cache.CacheStore.Get(payload.Entity, hash)
		if cached != nil {
			ctx.Response.Header.Set("Content-Type", "application/json")
			ctx.Response.Header.Set("X-Elysian-Cache", "HIT")
			ctx.SetStatusCode(fasthttp.StatusOK)
			ctx.SetBody(cached)
			return
		}
	}

	filter, err := ParseFilterNode(payload.Filters)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		return
	}

	query := api_storage.Query{
		Entity: payload.Entity,
		Offset: payload.Offset,
		Limit:  payload.Limit,
		Filter: filter,
		Sorts:  payload.Sorts,
	}

	data, err := api_storage.ExecuteQuery(query)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		return
	}

	data = acl.FilterListOfEntities(payload.Entity, data)

	if globals.GetConfig().Api.Hooks.Enabled && hook.EntityHasPreReadHooks(payload.Entity) {
		for i, item := range data {
			data[i] = hook.ApplyPreReadHooksForEntity(payload.Entity, item)
		}

		data = api_storage.ApplyQueryFilter(data, filter)
	}

	if globals.GetConfig().Api.Hooks.Enabled && hook.EntityHasPostReadHooks(payload.Entity) {
		for i, item := range data {
			data[i] = hook.ApplyPostReadHooksForEntity(payload.Entity, item)
		}
	}

	if payload.CountOnly {
		countResult := int64(len(data))
		response := []byte(`{"count":` + fmt.Sprintf("%d", countResult) + `}`)

		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
		ctx.SetStatusCode(fasthttp.StatusOK)
		ctx.SetBody(response)

		if globals.GetConfig().Api.Cache.Enabled {
			cache.CacheStore.Set(payload.Entity, hash, response)
		}

		return
	}

	fields := api_storage.ParseFieldsParam(payload.Fields)
	if len(fields) > 0 {
		filteredData := make([]map[string]any, len(data))
		for i, item := range data {
			filteredData[i] = api_storage.FilterFields(item, fields)
		}

		data = filteredData
	}

	responseBody, err := json.Marshal(data)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		return
	}

	ctx.SetBody(responseBody)
	ctx.SetStatusCode(fasthttp.StatusOK)

	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Set(payload.Entity, hash, responseBody)
	}
}

func ParseFilterNode(raw map[string]any) (api_storage.FilterNode, error) {
	if raw == nil {
		return api_storage.FilterNode{}, nil
	}

	if orRaw, ok := raw["or"]; ok {
		arr, ok := orRaw.([]any)
		if !ok {
			return api_storage.FilterNode{}, fmt.Errorf("or must be an array")
		}

		nodes := make([]api_storage.FilterNode, 0, len(arr))
		for _, item := range arr {
			m, ok := item.(map[string]any)
			if !ok {
				return api_storage.FilterNode{}, fmt.Errorf("or item must be an object")
			}

			n, err := ParseFilterNode(m)
			if err != nil {
				return api_storage.FilterNode{}, err
			}

			nodes = append(nodes, n)
		}

		return api_storage.FilterNode{Or: nodes}, nil
	}

	if andRaw, ok := raw["and"]; ok {
		arr, ok := andRaw.([]any)
		if !ok {
			return api_storage.FilterNode{}, fmt.Errorf("and must be an array")
		}

		nodes := make([]api_storage.FilterNode, 0, len(arr))
		for _, item := range arr {
			m, ok := item.(map[string]any)
			if !ok {
				return api_storage.FilterNode{}, fmt.Errorf("and item must be an object")
			}

			n, err := ParseFilterNode(m)
			if err != nil {
				return api_storage.FilterNode{}, err
			}

			nodes = append(nodes, n)
		}

		return api_storage.FilterNode{And: nodes}, nil
	}

	leaf := make(map[string]map[string]string)
	for field, v := range raw {
		opsRaw, ok := v.(map[string]any)
		if !ok {
			return api_storage.FilterNode{}, fmt.Errorf("invalid filter for field %s", field)
		}

		ops := make(map[string]string)
		for op, val := range opsRaw {
			s, ok := val.(string)
			if !ok {
				return api_storage.FilterNode{}, fmt.Errorf("invalid value for %s.%s", field, op)
			}

			ops[op] = s
		}

		leaf[field] = ops
	}

	return api_storage.FilterNode{Leaf: leaf}, nil
}
