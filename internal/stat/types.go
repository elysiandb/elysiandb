package stat

import (
	"encoding/json"
	"sync/atomic"

	"github.com/taymour/elysiandb/internal/globals"
)

type StatsContainer struct {
	keysCount           atomic.Uint64
	expirationKeysCount atomic.Uint64
	uptimeSeconds       atomic.Uint64
	totalRequests       atomic.Uint64
	hits                atomic.Uint64
	misses              atomic.Uint64
	version             string
	EntitiesCount       int
}

func NewStatsContainer() *StatsContainer {
	return &StatsContainer{version: globals.VERSION}
}

func (s *StatsContainer) IncrementKeysCount()                 { s.keysCount.Add(1) }
func (s *StatsContainer) IncrementExpirationKeysCount()       { s.expirationKeysCount.Add(1) }
func (s *StatsContainer) IncrementTotalRequests()             { s.totalRequests.Add(1) }
func (s *StatsContainer) IncrementUptimeSeconds()             { s.uptimeSeconds.Add(1) }
func (s *StatsContainer) IncrementHits()                      { s.hits.Add(1) }
func (s *StatsContainer) IncrementMisses()                    { s.misses.Add(1) }
func (s *StatsContainer) SetKeysCount(count uint64)           { s.keysCount.Store(count) }
func (s *StatsContainer) SetExpirationKeysCount(count uint64) { s.expirationKeysCount.Store(count) }

func (s *StatsContainer) DecrementKeysCount() {
	for {
		v := s.keysCount.Load()
		if v == 0 {
			return
		}
		if s.keysCount.CompareAndSwap(v, v-1) {
			return
		}
	}
}

func (s *StatsContainer) DecrementExpirationKeysCount() {
	for {
		v := s.expirationKeysCount.Load()
		if v == 0 {
			return
		}
		if s.expirationKeysCount.CompareAndSwap(v, v-1) {
			return
		}
	}
}

func (s *StatsContainer) Reset() {
	s.keysCount.Store(0)
	s.expirationKeysCount.Store(0)
	s.uptimeSeconds.Store(0)
	s.totalRequests.Store(0)
	s.hits.Store(0)
	s.misses.Store(0)
}

type statsDTO struct {
	KeysCount           uint64 `json:"keys_count,string"`
	ExpirationKeysCount uint64 `json:"expiration_keys_count,string"`
	UptimeSeconds       uint64 `json:"uptime_seconds,string"`
	TotalRequests       uint64 `json:"total_requests,string"`
	Hits                uint64 `json:"hits,string"`
	Misses              uint64 `json:"misses,string"`
	Version             string `json:"version"`
	EntitiesCount       int    `json:"entities_count,string"`
}

func (s *StatsContainer) ToJson() string {
	dto := statsDTO{
		KeysCount:           s.keysCount.Load(),
		ExpirationKeysCount: s.expirationKeysCount.Load(),
		UptimeSeconds:       s.uptimeSeconds.Load(),
		TotalRequests:       s.totalRequests.Load(),
		Hits:                s.hits.Load(),
		Misses:              s.misses.Load(),
		Version:             s.version,
		EntitiesCount:       s.EntitiesCount,
	}
	b, _ := json.Marshal(dto)
	return string(b)
}
