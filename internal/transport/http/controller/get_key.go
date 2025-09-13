package controller

import (
	"encoding/json"
	"net/http"
	"net/url"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/wildcard"
	"github.com/valyala/fasthttp"
)

type getEntry struct {
	Key string  `json:"key"`
	Val *string `json:"value"`
}

func GetKeyController(ctx *fasthttp.RequestCtx) {
	cfg := globals.GetConfig()
	if cfg.Stats.Enabled {
		stat.Stats.IncrementTotalRequests()
	}

	ctx.SetContentType("text/plain; charset=utf-8")

	key := ctx.UserValue("key").(string)

	if dec, err := url.PathUnescape(key); err == nil {
		key = dec
	}

	if wildcard.KeyContainsWildcard(key) {
		handleWildcardKey(key, ctx)
	} else {
		handleSingleKey(key, ctx)
	}

	ctx.Response.Header.Set("Content-Type", "application/json")
}

func handleSingleKey(key string, ctx *fasthttp.RequestCtx) {
	cfg := globals.GetConfig()
	if storage.KeyHasExpired(key) {
		storage.DeleteByKey(key)

		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		jsonData, _ := json.Marshal(getEntry{
			Key: key,
			Val: nil,
		})
		_, _ = ctx.Write(jsonData)
		ctx.SetStatusCode(http.StatusNotFound)

		return
	}

	data, err := storage.GetByKey(key)
	if err != nil {
		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		jsonData, _ := json.Marshal(getEntry{
			Key: key,
			Val: nil,
		})
		_, _ = ctx.Write(jsonData)
		ctx.SetStatusCode(http.StatusNotFound)

		return
	}

	if cfg.Stats.Enabled {
		stat.Stats.IncrementHits()
	}

	valStr := string(data)
	jsonData, _ := json.Marshal(getEntry{
		Key: key,
		Val: &valStr,
	})

	_, _ = ctx.Write(jsonData)
}

func handleWildcardKey(key string, ctx *fasthttp.RequestCtx) {
	cfg := globals.GetConfig()
	var results = make([]multiGetEntry, 0)
	data := storage.GetByWildcardKey(key)

	if len(data) == 0 {
		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		results = append(results, multiGetEntry{
			Key: key,
			Val: nil,
		})

		jsonData, _ := json.Marshal(results)
		_, _ = ctx.Write(jsonData)

		return
	}

	for k, v := range data {
		valStr := string(v)
		results = append(results, multiGetEntry{
			Key: k,
			Val: &valStr,
		})

		if cfg.Stats.Enabled {
			stat.Stats.IncrementHits()
		}
	}

	jsonData, _ := json.Marshal(results)
	_, _ = ctx.Write(jsonData)
}
