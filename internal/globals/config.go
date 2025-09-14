package globals

import (
	"sync"

	"github.com/taymour/elysiandb/internal/configuration"
)

var (
	mu  sync.RWMutex
	cfg *configuration.Config
)

func SetConfig(c *configuration.Config) {
	mu.Lock()
	cfg = c
	mu.Unlock()
}

func GetConfig() *configuration.Config {
	mu.RLock()
	c := cfg
	mu.RUnlock()

	return c
}
