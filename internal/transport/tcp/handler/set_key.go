package handler

import (
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/tcp/parsing"
)

func HandleSet(query []byte, ttl int) []byte {
	if globals.GetConfig().Stats.Enabled {
		stat.Stats.IncrementTotalRequests()
	}

	k, v := parsing.FirstWordBytes(query)

	key := string(k)
	val := make([]byte, len(v))
	copy(val, v)

	var err error
	if ttl > 0 {
		err = storage.PutKeyValueWithTTL(key, val, ttl)
	} else {
		err = storage.PutKeyValue(key, val)
	}

	if err != nil {
		log.Error("set failed", "component", "tcp", "err", err)
		return []byte("ERR")
	}

	return []byte("OK")
}
