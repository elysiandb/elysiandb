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
	"github.com/taymour/elysiandb/internal/engine"
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
	return GetJsonByKey(key)
}

func GetJsonRaw(key string) ([]byte, error) {
	js := mainJsonStore.Load()
	if js == nil {
		return nil, fmt.Errorf("json store not initialized")
	}
	raw, ok := js.getRaw(key)
	if !ok {
		return nil, fmt.Errorf("key not found: %s", key)
	}
	return raw, nil
}

func GetJsonByKey(key string) (map[string]interface{}, error) {
	js := mainJsonStore.Load()
	if js == nil {
		return nil, fmt.Errorf("json store not initialized")
	}

	raw, ok := js.getRaw(key)
	if !ok {
		return nil, fmt.Errorf("key not found: %s", key)
	}

	var out map[string]interface{}
	if err := json.Unmarshal(raw, &out); err != nil {
		return nil, fmt.Errorf("error decoding json for key %s: %w", key, err)
	}

	cp := make(map[string]interface{}, len(out))
	for k, v := range out {
		cp[k] = v
	}

	return cp, nil
}

func PutJsonValue(key string, value map[string]interface{}) error {
	cfg := globals.GetConfig()
	js := mainJsonStore.Load()
	if js == nil {
		return fmt.Errorf("json store not initialized")
	}

	_, existed := js.getRaw(key)

	data, err := json.Marshal(value)
	if err != nil {
		return fmt.Errorf("error encoding json value: %w", err)
	}

	js.putRaw(key, data)

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
	_, existed := js.getRaw(key)
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
		sh.m.Range(func(k, _ any) bool {
			if strings.HasPrefix(k.(string), pre) {
				sh.m.Delete(k)
				deleted++
			}
			return true
		})
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
	m  sync.Map
	ar *engine.Arena
}

type JsonStore struct {
	shards     []*jsonShard
	saved      atomic.Bool
	shardMask  uint64
	shardCount int
}

func NewJsonStore() *JsonStore {
	cfg := globals.GetConfig()
	n := cfg.Store.Shards
	chunkSize := cfg.Store.Json.ArenaChunkSize

	s := &JsonStore{
		shards:     make([]*jsonShard, n),
		shardMask:  uint64(n - 1),
		shardCount: n,
	}
	for i := 0; i < n; i++ {
		s.shards[i] = &jsonShard{
			m:  sync.Map{},
			ar: engine.NewArena(chunkSize),
		}
	}
	s.saved.Store(true)
	return s
}

func (s *JsonStore) CountTotalKeys() uint64 {
	total := uint64(0)
	for i := 0; i < s.shardCount; i++ {
		c := uint64(0)
		s.shards[i].m.Range(func(_, _ any) bool {
			c++
			return true
		})
		total += c
	}
	return total
}

func (s *JsonStore) reset() {
	for i := 0; i < s.shardCount; i++ {
		s.shards[i].m = sync.Map{}
		s.shards[i].ar.Reset()
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

func (s *JsonStore) getRaw(key string) ([]byte, bool) {
	sh := s.shards[s.shardIndex(key)]
	v, ok := sh.m.Load(key)
	if !ok {
		return nil, false
	}
	return v.([]byte), true
}

func (s *JsonStore) putRaw(key string, data []byte) {
	sh := s.shards[s.shardIndex(key)]

	buf := sh.ar.Alloc(len(data))
	copy(buf, data)

	sh.m.Store(key, buf)
	s.saved.Store(false)

	if globals.GetConfig().Store.CrashRecovery.Enabled {
		var decoded map[string]interface{}
		if err := json.Unmarshal(buf, &decoded); err == nil {
			recovery.LogJsonPut(key, decoded)
		}
	}
}

func (s *JsonStore) del(key string) {
	sh := s.shards[s.shardIndex(key)]
	sh.m.Delete(key)
	s.saved.Store(false)
	if globals.GetConfig().Store.CrashRecovery.Enabled {
		recovery.LogJsonDelete(key)
	}
}

func (s *JsonStore) Iterate(fn func(k string, v map[string]interface{})) {
	for i := 0; i < s.shardCount; i++ {
		s.shards[i].m.Range(func(k, v any) bool {
			raw := v.([]byte)
			var decoded map[string]interface{}
			if err := json.Unmarshal(raw, &decoded); err == nil {
				fn(k.(string), decoded)
			}
			return true
		})
	}
}

func (s *JsonStore) FromMap(src map[string]map[string]interface{}) {
	for k, v := range src {
		data, err := json.Marshal(v)
		if err != nil {
			continue
		}
		s.putRaw(k, data)
	}
	s.saved.Store(true)
}

func (s *JsonStore) ToMap() map[string]map[string]interface{} {
	result := make(map[string]map[string]interface{})
	s.Iterate(func(k string, v map[string]interface{}) {
		cp := make(map[string]interface{}, len(v))
		for fk, fv := range v {
			cp[fk] = fv
		}
		result[k] = cp
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
