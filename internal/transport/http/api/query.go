package api

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"sort"
	"strconv"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
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
	SortOrder []string          `json:"-"`
}

func (q *QueryPayload) UnmarshalJSON(data []byte) error {
	type rawPayload QueryPayload
	var payload rawPayload
	if err := json.Unmarshal(data, &payload); err != nil {
		return err
	}

	var raw struct {
		Sorts json.RawMessage `json:"sorts"`
	}
	_ = json.Unmarshal(data, &raw)

	*q = QueryPayload(payload)
	q.SortOrder = objectKeysInOrder(raw.Sorts)

	return nil
}

func objectKeysInOrder(raw json.RawMessage) []string {
	dec := json.NewDecoder(bytes.NewReader(raw))
	if token, err := dec.Token(); err != nil || token != json.Delim('{') {
		return nil
	}

	var keys []string
	for dec.More() {
		token, _ := dec.Token()
		key, _ := token.(string)
		keys = append(keys, key)

		var value json.RawMessage
		_ = dec.Decode(&value)
	}

	return keys
}

func (q *QueryPayload) Hash() []byte {
	normalized := map[string]any{
		"entity":    q.Entity,
		"offset":    q.Offset,
		"limit":     q.Limit,
		"filters":   normalizeAny(q.Filters),
		"sorts":     normalizeStringMap(q.Sorts),
		"sortOrder": q.queryForSorts().OrderedSorts(),
		"countOnly": q.CountOnly,
		"fields":    q.Fields,
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

func (q *QueryPayload) queryForSorts() query.Query {
	return query.Query{Sorts: q.Sorts, SortOrder: q.SortOrder}
}

func sendQueryError(ctx *fasthttp.RequestCtx, status int, err error) {
	body, _ := json.Marshal(map[string]string{"error": err.Error()})
	ctx.SetStatusCode(status)
	ctx.SetBody(body)
}

func QueryController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	var payload QueryPayload
	if err := json.Unmarshal(ctx.PostBody(), &payload); err != nil {
		sendQueryError(ctx, fasthttp.StatusBadRequest, err)
		return
	}

	if engine.IsCoreEntityType(payload.Entity) {
		sendCoreEntityForbidden(ctx)
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
		currentUser = security.GetCurrentUsername(ctx)
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
		sendQueryError(ctx, fasthttp.StatusBadRequest, err)
		return
	}

	preReadHooks := globals.GetConfig().Api.Hooks.Enabled && hook.EntityHasPreReadHooks(payload.Entity)

	q := query.Query{
		Entity:    payload.Entity,
		Offset:    payload.Offset,
		Limit:     payload.Limit,
		Filter:    filter,
		Sorts:     payload.Sorts,
		SortOrder: payload.SortOrder,
	}

	filterAfterHooks := preReadHooks && !api_storage.QueryFilterIsEmpty(filter)
	if filterAfterHooks {
		q.Offset = 0
		q.Limit = 0
		q.Filter = query.FilterNode{Leaf: map[string]map[string]string{}}
	}

	data, err := engine.ExecuteQuery(q)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		return
	}

	if filterAfterHooks {
		data = api_storage.ApplyStoredQueryFilter(data, filter)
	}

	data = acl.FilterListOfEntities(currentUser, payload.Entity, data)

	if filterAfterHooks {
		matches := func(item map[string]any) bool { return api_storage.QueryFilterMatchesEntity(filter, item) }
		data = applyPreReadHooksAndFilter(currentUser, payload.Entity, data, matches, payload.Offset, payload.Limit)
	} else if preReadHooks {
		for i, item := range data {
			data[i] = hook.ApplyPreReadHooksForEntity(currentUser, payload.Entity, item)
		}
	}

	if globals.GetConfig().Api.Hooks.Enabled && hook.EntityHasPostReadHooks(payload.Entity) {
		for i, item := range data {
			data[i] = hook.ApplyPostReadHooksForEntity(currentUser, payload.Entity, item)
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
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		return
	}

	ctx.SetBody(responseBody)
	ctx.SetStatusCode(fasthttp.StatusOK)

	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Set(payload.Entity, hash, responseBody)
	}
}

func filterValueToString(val any) (string, bool) {
	switch v := val.(type) {
	case string:
		return v, true
	case float64:
		return strconv.FormatFloat(v, 'f', -1, 64), true
	case bool:
		return strconv.FormatBool(v), true
	default:
		return "", false
	}
}

func ParseFilterNode(raw map[string]any) (query.FilterNode, error) {
	if raw == nil {
		return query.FilterNode{}, nil
	}

	if orRaw, ok := raw["or"]; ok {
		arr, ok := orRaw.([]any)
		if !ok {
			return query.FilterNode{}, fmt.Errorf("or must be an array")
		}

		nodes := make([]query.FilterNode, 0, len(arr))
		for _, item := range arr {
			m, ok := item.(map[string]any)
			if !ok {
				return query.FilterNode{}, fmt.Errorf("or item must be an object")
			}

			n, err := ParseFilterNode(m)
			if err != nil {
				return query.FilterNode{}, err
			}

			nodes = append(nodes, n)
		}

		return query.FilterNode{Or: nodes}, nil
	}

	if andRaw, ok := raw["and"]; ok {
		arr, ok := andRaw.([]any)
		if !ok {
			return query.FilterNode{}, fmt.Errorf("and must be an array")
		}

		nodes := make([]query.FilterNode, 0, len(arr))
		for _, item := range arr {
			m, ok := item.(map[string]any)
			if !ok {
				return query.FilterNode{}, fmt.Errorf("and item must be an object")
			}

			n, err := ParseFilterNode(m)
			if err != nil {
				return query.FilterNode{}, err
			}

			nodes = append(nodes, n)
		}

		return query.FilterNode{And: nodes}, nil
	}

	leaf := make(map[string]map[string]string)
	for field, v := range raw {
		opsRaw, ok := v.(map[string]any)
		if !ok {
			return query.FilterNode{}, fmt.Errorf("invalid filter for field %s", field)
		}

		ops := make(map[string]string)
		for op, val := range opsRaw {
			s, ok := filterValueToString(val)
			if !ok {
				return query.FilterNode{}, fmt.Errorf("invalid value for %s.%s: expected a string, a number or a boolean", field, op)
			}

			ops[op] = s
		}

		leaf[field] = ops
	}

	return query.FilterNode{Leaf: leaf}, nil
}
