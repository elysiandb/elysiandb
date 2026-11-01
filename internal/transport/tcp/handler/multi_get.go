package handler

import (
	"fmt"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/tcp/parsing"
	"github.com/taymour/elysiandb/internal/wildcard"
)

func HandleMultiGet(query []byte) []byte {
	cfg := globals.GetConfig()
	if cfg.Stats.Enabled {
		stat.Stats.IncrementTotalRequests()
	}

	keys := strings.Split(string(query), " ")
	if len(keys) == 0 {
		return []byte("ERR")
	}

	results := make([][]byte, len(keys))
	for i, key := range keys {
		if wildcard.KeyContainsWildcard(key) {
			HandleMGETWildcardKey(key, i, &results)
		} else {
			HandleMGETSingleKey(key, i, &results)
		}
	}

	return parsing.JoinByteSlices(results, []byte("\n"))
}

func HandleMGETSingleKey(key string, i int, results *[][]byte) {
	cfg := globals.GetConfig()
	if storage.KeyHasExpired(key) {
		storage.DeleteByKey(key)

		metrics.KVMiss()
		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		(*results)[i] = []byte(fmt.Sprintf("%s=not found", key))

		return
	}

	data, err := storage.GetByKey(key)
	if err != nil {
		metrics.KVMiss()
		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		(*results)[i] = []byte(fmt.Sprintf("%s=not found", key))

		return
	}

	(*results)[i] = data

	metrics.KVHit()
	if cfg.Stats.Enabled {
		stat.Stats.IncrementHits()
	}
}

func HandleMGETWildcardKey(key string, i int, results *[][]byte) {
	cfg := globals.GetConfig()
	data := storage.GetByWildcardKey(key)
	if len(data) == 0 {
		metrics.KVMiss()
		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		(*results)[i] = []byte(fmt.Sprintf("%s=not found", key))
		return
	}

	result := make([]byte, 0)
	for k, v := range data {
		result = append(result, []byte(k+"="+string(v)+"\n")...)

		metrics.KVHit()
		if cfg.Stats.Enabled {
			stat.Stats.IncrementHits()
		}
	}

	if len(result) > 0 {
		result = result[:len(result)-1]
	}

	(*results)[i] = result
}
