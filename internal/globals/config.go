package globals

import (
	"sync/atomic"

	"github.com/taymour/elysiandb/internal/configuration"
)

var cfg atomic.Pointer[configuration.Config]

func SetConfig(c *configuration.Config) {
	cfg.Store(c)
}

func GetConfig() *configuration.Config {
	return cfg.Load()
}

func GetEngine() string {
	c := GetConfig()
	if c.Engine.Name == "" {
		return "internal"
	}

	return c.Engine.Name
}
