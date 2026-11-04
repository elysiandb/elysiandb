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
	autoMemoryShare  = 0.75
	blockCacheShare  = 0.30
	memTableDivisor  = 64
	buildDivisor     = 128
	minBuildBytes    = 4 << 20
	maxBuildBytes    = 64 << 20
	unlimitedCgroups = 1 << 60
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

type MemorySplit struct {
	Budget   int64
	Cache    int64
	MemTable int64
	Build    int64
}

func SplitMemory(cfg *configuration.Config, budget int64) MemorySplit {
	return MemorySplit{
		Budget:   budget,
		Cache:    int64(float64(budget) * blockCacheShare),
		MemTable: storage.LimitMemTable(cfg, budget/memTableDivisor),
		Build:    max(min(budget/buildDivisor, maxBuildBytes), minBuildBytes),
	}
}

func ApplyMemoryLimit() {
	cfg := globals.GetConfig()
	budget, explicit := MemoryBudget(cfg)
	if budget <= 0 {
		memory.SetBudget(0)
		return
	}

	if os.Getenv("GOMEMLIMIT") == "" {
		debug.SetMemoryLimit(budget)
	}

	split := SplitMemory(cfg, budget)
	storage.CacheBytes = func(*configuration.Config) int64 { return split.Cache }
	storage.MemTableBytes = func(*configuration.Config) int64 { return split.MemTable }
	storage.BuildBytes = func(*configuration.Config) int64 { return split.Build }
	memory.SetBudget(budget)
	memory.Start()

	bootLog.Info("memory budget applied",
		"limit_mb", budget>>20,
		"explicit", explicit,
		"block_cache_mb", split.Cache>>20,
		"memtable_mb", split.MemTable>>20,
		"index_build_mb", split.Build>>20,
	)
}
