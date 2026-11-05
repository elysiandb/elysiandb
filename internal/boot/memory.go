package boot

import (
	"os"
	"runtime/debug"
	"strconv"
	"strings"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/memory"
	"github.com/taymour/elysiandb/internal/storage"
)

const (
	autoMemoryShare   = 0.75
	heapShareOfBudget = 0.85
	unlimitedCgroups  = 1 << 60
)

var cgroupLimitFiles = []string{"/sys/fs/cgroup/memory.max", "/sys/fs/cgroup/memory/memory.limit_in_bytes"}

func containerMemoryLimit() int64 {
	for _, path := range cgroupLimitFiles {
		data, err := os.ReadFile(path)
		if err != nil {
			continue
		}

		v := strings.TrimSpace(string(data))
		if v == "max" {
			return 0
		}

		n, err := strconv.ParseInt(v, 10, 64)
		if err == nil && n > 0 && n < unlimitedCgroups {
			return n
		}
	}

	return 0
}

func MemoryBudget(cfg *configuration.Config) (int64, bool) {
	if cfg.Store.Memory.LimitMB > 0 {
		return cfg.Store.Memory.LimitMB << 20, true
	}

	if limit := containerMemoryLimit(); limit > 0 {
		return int64(float64(limit) * autoMemoryShare), false
	}

	return 0, false
}

func ApplyMemoryLimit() {
	cfg := globals.GetConfig()
	budget, explicit := MemoryBudget(cfg)
	if !explicit {
		memory.Configure(0)
	}

	if budget <= 0 {
		return
	}

	if os.Getenv("GOMEMLIMIT") == "" {
		limit := budget
		if explicit {
			limit = int64(float64(budget) * heapShareOfBudget)
		}
		debug.SetMemoryLimit(limit)
	}

	if explicit {
		memory.Configure(budget)
		memory.Start()
		storage.ValueCacheBytes = func(*configuration.Config) int64 {
			return memory.Values.Limit()
		}
		storage.PageCacheBytes = func(*configuration.Config) int64 {
			return memory.Pages.Limit()
		}
		storage.DirtyFlushBytes = func(*configuration.Config) int64 {
			return memory.Dirty.Limit()
		}
	}

	bootLog.Info("memory budget applied", "limit_mb", budget>>20, "explicit", explicit, "documents_cached_on_demand", explicit)
}
