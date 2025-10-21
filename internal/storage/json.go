package storage

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"sync"
	"sync/atomic"

	xxhash "github.com/cespare/xxhash/v2"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/stat"
)

var mainJsonStore atomic.Pointer[JsonStore]

func LoadJsonDB() {
	cfg := globals.GetConfig()
	createFolder(cfg.Store.Folder)
	createFile(cfg.Store.Folder, JsonDataFile)
	js := createJsonStore(JsonDataFile)
	mainJsonStore.Store(js)
}

func createJsonStore(file string) *JsonStore {
	data, err := ReadJsonFromDB(file)
	if err != nil {
		log.Fatal("Error loading json database:", err)
	}
	newStore := NewJsonStore()
	newStore.FromMap(data)
	newStore.saved.Store(true)
	return newStore
}

func GetJsonByKeyNoCopy(key string) (map[string]interface{}, error) {
	js := mainJsonStore.Load()
	if js == nil {
		return nil, fmt.Errorf("json store not initialized")
	}
	val, ok := js.get(key)
	if !ok {
		return nil, fmt.Errorf("key not found: %s", key)
	}
	return val, nil
}

func GetJsonByKey(key string) (map[string]interface{}, error) {
	v, err := GetJsonByKeyNoCopy(key)
	if err != nil {
		return nil, err
	}
	cp := make(map[string]interface{}, len(v))
	for k, x := range v {
		cp[k] = x
	}
	return cp, nil
}

func PutJsonValue(key string, value map[string]interface{}) error {
	cfg := globals.GetConfig()
	js := mainJsonStore.Load()
	if js == nil {
		return fmt.Errorf("json store not initialized")
	}
	_, existed := js.get(key)
	js.put(key, value)
	if cfg.Stats.Enabled && !existed {
		stat.Stats.IncrementKeysCount()
	}
	return nil
}

func DeleteJsonByKey(key string) {
	cfg := globals.GetConfig()
	js := mainJsonStore.Load()
	if js == nil {
		return
	}
	_, existed := js.get(key)
	js.del(key)
	if cfg.Stats.Enabled && existed {
		stat.Stats.DecrementKeysCount()
	}
}

func DeleteJsonByPrefix(prefix string) int {
	cfg := globals.GetConfig()
	js := mainJsonStore.Load()
	if js == nil {
		return 0
	}
	pre := strings.TrimSuffix(prefix, "*")
	deleted := 0
	for _, sh := range js.shards {
		for k := range sh.m {
			if strings.HasPrefix(k, pre) {
				delete(sh.m, k)
				deleted++
			}
		}
	}
	js.saved.Store(false)
	if cfg.Stats.Enabled {
		for i := 0; i < deleted; i++ {
			stat.Stats.DecrementKeysCount()
		}
	}
	return deleted
}

type jsonShard struct {
	mu sync.RWMutex
	m  map[string]map[string]interface{}
}

type JsonStore struct {
	shards     []*jsonShard
	saved      atomic.Bool
	shardMask  uint64
	shardCount int
}

func NewJsonStore() *JsonStore {
	n := globals.GetConfig().Store.Shards
	s := &JsonStore{
		shards:     make([]*jsonShard, n),
		shardMask:  uint64(n - 1),
		shardCount: n,
	}
	for i := 0; i < n; i++ {
		s.shards[i] = &jsonShard{m: make(map[string]map[string]interface{})}
	}
	s.saved.Store(true)
	return s
}

func (s *JsonStore) CountTotalKeys() uint64 {
	total := uint64(0)
	for i := 0; i < s.shardCount; i++ {
		sh := s.shards[i]
		sh.mu.RLock()
		total += uint64(len(sh.m))
		sh.mu.RUnlock()
	}
	return total
}

func (s *JsonStore) reset() {
	for i := 0; i < s.shardCount; i++ {
		sh := s.shards[i]
		sh.mu.Lock()
		sh.m = make(map[string]map[string]interface{})
		sh.mu.Unlock()
	}
	s.saved.Store(false)
	if globals.GetConfig().Store.CrashRecovery.Enabled {
		recovery.ClearJsonRecoveryLog()
	}
}

func (s *JsonStore) shardIndex(key string) int {
	h := xxhash.Sum64String(key)
	return int(h & s.shardMask)
}

func (s *JsonStore) get(key string) (map[string]interface{}, bool) {
	sh := s.shards[s.shardIndex(key)]
	sh.mu.RLock()
	v, ok := sh.m[key]
	sh.mu.RUnlock()
	if !ok {
		return nil, false
	}
	return v, true
}

func (s *JsonStore) put(key string, value map[string]interface{}) {
	sh := s.shards[s.shardIndex(key)]
	sh.mu.Lock()
	sh.m[key] = value
	sh.mu.Unlock()
	s.saved.Store(false)
	if globals.GetConfig().Store.CrashRecovery.Enabled {
		recovery.LogJsonPut(key, value)
	}
}

func (s *JsonStore) del(key string) {
	sh := s.shards[s.shardIndex(key)]
	sh.mu.Lock()
	delete(sh.m, key)
	sh.mu.Unlock()
	s.saved.Store(false)
	if globals.GetConfig().Store.CrashRecovery.Enabled {
		recovery.LogJsonDelete(key)
	}
}

func (s *JsonStore) Iterate(fn func(k string, v map[string]interface{})) {
	for i := 0; i < s.shardCount; i++ {
		sh := s.shards[i]
		sh.mu.RLock()
		for k, v := range sh.m {
			fn(k, v)
		}
		sh.mu.RUnlock()
	}
}

func (s *JsonStore) FromMap(src map[string]map[string]interface{}) {
	for k, v := range src {
		sh := s.shards[s.shardIndex(k)]
		sh.mu.Lock()
		sh.m[k] = v
		sh.mu.Unlock()
	}
	s.saved.Store(true)
}

func (s *JsonStore) ToMap() map[string]map[string]interface{} {
	result := make(map[string]map[string]interface{})
	s.Iterate(func(k string, v map[string]interface{}) {
		result[k] = v
	})
	return result
}

func writeJsonStoreToFile(cfg *configuration.Config, fileName string, store *JsonStore) error {
	if store.saved.Load() {
		return nil
	}
	isSuccess := true
	path := cfg.Store.Folder + "/" + fileName
	file, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o644)
	if err != nil {
		isSuccess = false
		log.Error("Error opening file:", err)
	}
	defer file.Close()
	encoder := json.NewEncoder(file)
	storeAsMap := store.ToMap()
	if err := encoder.Encode(storeAsMap); err != nil {
		isSuccess = false
		log.Error("Error encoding JSON:", err)
	}
	store.saved.Store(isSuccess)
	return nil
}
