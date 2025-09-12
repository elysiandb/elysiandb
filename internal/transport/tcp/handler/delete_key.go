package handler

import (
	"fmt"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/wildcard"
)

func HandleDelete(query []byte) []byte {
	if globals.GetConfig().Stats.Enabled {
		stat.Stats.IncrementTotalRequests()
	}

	key := string(query)

	var count int
	if wildcard.KeyContainsWildcard(key) {
		count = handleWildcardKeyDelete(key)
	} else {
		count = handleSingleKeyDelete(key)
	}

	return []byte(fmt.Sprintf("Deleted %d", count))
}

func handleSingleKeyDelete(key string) int {
	storage.DeleteByKey(key)
	return 1
}

func handleWildcardKeyDelete(pattern string) int {
	return storage.DeleteByWildcardKey(pattern)
}
