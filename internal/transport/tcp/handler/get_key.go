package handler

import (
	"fmt"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/wildcard"
)

func HandleGet(query []byte) []byte {
	cfg := globals.GetConfig()
	if cfg.Stats.Enabled {
		stat.Stats.IncrementTotalRequests()
	}

	if wildcard.KeyContainsWildcard(string(query)) {
		return handleWildcardKey(query)
	}

	return handleSingleKey(query)
}

func handleWildcardKey(query []byte) []byte {
	cfg := globals.GetConfig()
	data := storage.GetByWildcardKey(string(query))
	if len(data) == 0 {
		metrics.KVMiss()
		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		return []byte(fmt.Sprintf("%s=not found", string(query)))
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

	return result
}

func handleSingleKey(query []byte) []byte {
	cfg := globals.GetConfig()

	key := string(query)

	if storage.KeyHasExpired(key) {
		storage.DeleteByKey(key)

		metrics.KVMiss()
		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		return []byte(fmt.Sprintf("%s=not found", key))
	}

	data, err := storage.GetByKey(key)
	if err != nil {
		metrics.KVMiss()
		if cfg.Stats.Enabled {
			stat.Stats.IncrementMisses()
		}

		return []byte(fmt.Sprintf("%s=not found", key))
	}

	metrics.KVHit()
	if cfg.Stats.Enabled {
		stat.Stats.IncrementHits()
	}

	return []byte(fmt.Sprintf("%s=%s", key, data))
}
