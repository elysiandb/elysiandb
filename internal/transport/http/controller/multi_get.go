package controller

import (
	"encoding/json"
	"net/http"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/wildcard"
	"github.com/valyala/fasthttp"
)

type multiGetEntry struct {
	Key string  `json:"key"`
	Val *string `json:"value"`
}

func MultiGetController(ctx *fasthttp.RequestCtx) {
	cfg := globals.GetConfig()
	if cfg.Stats.Enabled {
		stat.Stats.IncrementTotalRequests()
	}

	ctx.SetContentType("application/json; charset=utf-8")

	raw := strings.TrimSpace(string(ctx.QueryArgs().Peek("keys")))
	if raw == "" {
		_, _ = ctx.Write([]byte("[]"))
		return
	}

	parts := strings.Split(raw, ",")
	keys := make([]string, 0, len(parts))
	for _, p := range parts {
		p = strings.TrimSpace(p)
		if p != "" {
			keys = append(keys, p)
		}
	}
	if len(keys) == 0 {
		_, _ = ctx.Write([]byte("[]"))
		return
	}

	results := make([]multiGetEntry, 0, len(keys))
	seen := make(map[string]struct{}, len(keys))

	for _, key := range keys {
		if wildcard.KeyContainsWildcard(key) {
			handleMGETWildcardKey(key, &results, seen)
		} else {
			handleMGETSingleKey(key, &results, seen)
		}
	}

	jsonData, err := json.Marshal(results)
	if err != nil {
		ctx.Error(err.Error(), http.StatusInternalServerError)
		return
	}

	_, _ = ctx.Write(jsonData)
}

func handleMGETSingleKey(key string, results *[]multiGetEntry, seen map[string]struct{}) {
	if _, dup := seen[key]; dup {
		return
	}

	seen[key] = struct{}{}

	cfg := globals.GetConfig()

	if storage.KeyHasExpired(key) {
		storage.DeleteByKey(key)
		*results = append(*results, multiGetEntry{Key: key, Val: nil})

		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		return
	}

	data, err := storage.GetByKey(key)
	if err != nil {
		*results = append(*results, multiGetEntry{Key: key, Val: nil})

		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		return
	}

	val := string(data)
	*results = append(*results, multiGetEntry{Key: key, Val: &val})

	if cfg.Stats.Enabled {
		stat.Stats.IncrementHits()
	}
}

func handleMGETWildcardKey(pattern string, results *[]multiGetEntry, seen map[string]struct{}) {
	cfg := globals.GetConfig()

	data := storage.GetByWildcardKey(pattern)
	for k, v := range data {
		if _, dup := seen[k]; dup {
			continue
		}

		seen[k] = struct{}{}

		valStr := string(v)
		*results = append(*results, multiGetEntry{
			Key: k,
			Val: &valStr,
		})

		if cfg.Stats.Enabled {
			stat.Stats.IncrementHits()
		}
	}
}
