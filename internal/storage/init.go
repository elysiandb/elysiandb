package storage

import (
	"fmt"
	"maps"
	"os"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/stat"
)

var mainStore *Store
var expirationContainer *ExpirationContainer
var rootMu sync.RWMutex

func LoadDB() {
	cfg := globals.GetConfig()

	createFolder(cfg.Store.Folder)
	createFile(cfg.Store.Folder, DataFile)
	createFile(cfg.Store.Folder, ExpirationDataFile)

	ms := createStore(DataFile)
	ec := createExpirationContainer(ExpirationDataFile)

	rootMu.Lock()
	mainStore = ms
	expirationContainer = ec
	rootMu.Unlock()

	CleanAllPastKeys()

	if cfg.Stats.Enabled {
		stat.Stats.SetKeysCount(mainStore.CountTotalKeys())
		stat.Stats.SetExpirationKeysCount(expirationContainer.CountTotalKeys())
	}
}

func createExpirationContainer(fileName string) *ExpirationContainer {
	container := newExpirationContainer()

	data, err := ReadExpirationsFromDB(fileName)
	if err != nil {
		log.Fatal("Error loading expiration database:", err)
	}

	for ts, keys := range data {
		container.put(ts, keys)
	}

	return container
}

func createStore(file string) *Store {
	data, err := ReadFromDB(file)
	if err != nil {
		log.Fatal("Error loading database:", err)
	}

	bytesData := make(map[string][]byte, len(data))
	maps.Copy(bytesData, data)

	newStore := NewStore()
	newStore.FromMap(bytesData)
	newStore.saved.Store(true)

	return newStore
}

func createFolder(folder string) {
	if err := os.MkdirAll(folder, 0755); err != nil {
		log.Fatal("Error creating data folder:", err)
	}
}

func createFile(folder string, file string) {
	filePath := folder + "/" + file
	if _, err := os.Stat(filePath); os.IsNotExist(err) {
		file, err := os.Create(filePath)
		if err != nil {
			log.Fatal("Error creating data file:", err)
		}
		file.WriteString("{}")
		defer file.Close()
	}
}

func GetByKey(key string) ([]byte, error) {
	rootMu.RLock()
	ms := mainStore
	rootMu.RUnlock()
	if val, ok := ms.get(key); ok {
		return val, nil
	}

	return nil, fmt.Errorf("key not found: %s", key)
}

func GetByWildcardKeyWithLimit(pattern string, limit int) map[string][]byte {
	out := make(map[string][]byte)
	keys := make([]string, 0)

	if isBareStar(pattern) {
		mainStore.Iterate(func(k string, v []byte) {
			keys = append(keys, k)
			limit--
			if limit == 0 {
				return
			}
		})
	} else {
		mainStore.Iterate(func(k string, v []byte) {
			if matchGlob(pattern, k) {
				keys = append(keys, k)
				limit--
				if limit == 0 {
					return
				}
			}
		})
	}

	for _, k := range keys {
		if KeyHasExpired(k) {
			DeleteByKey(k)
			continue
		}

		value, err := GetByKey(k)
		if err != nil {
			continue
		}

		out[k] = value
	}

	return out
}

func GetByWildcardKey(pattern string) map[string][]byte {
	return GetByWildcardKeyWithLimit(pattern, -1)
}

func DeleteByWildcardKey(pattern string) int {
	keys := make([]string, 0)

	if isBareStar(pattern) {
		mainStore.Iterate(func(k string, v []byte) {
			keys = append(keys, k)
		})
	} else {
		mainStore.Iterate(func(k string, v []byte) {
			if matchGlob(pattern, k) {
				keys = append(keys, k)
			}
		})
	}

	for _, k := range keys {
		DeleteByKey(k)
	}

	return len(keys)
}

func PutKeyValue(key string, value []byte) error {
	return PutKeyValueWithTTL(key, value, -1)
}

func PutKeyValueWithTTL(key string, value []byte, ttl int) error {
	cfg := globals.GetConfig()
	_, existed := mainStore.get(key)
	hadTTL := hasTTL(key)

	mainStore.put(key, value)

	if ttl > 0 {
		expiration := time.Now().Unix() + int64(ttl)
		expirationContainer.put(expiration, []string{key})
		if cfg.Stats.Enabled && !hadTTL {
			stat.Stats.IncrementExpirationKeysCount()
		}
	}

	if cfg.Stats.Enabled && !existed {
		stat.Stats.IncrementKeysCount()
	}

	return nil
}

func DeleteByKey(key string) {
	cfg := globals.GetConfig()

	_, existed := mainStore.get(key)
	hadTTL := hasTTL(key)

	mainStore.del(key)
	expirationContainer.del(key)

	if cfg.Stats.Enabled {
		if existed {
			stat.Stats.DecrementKeysCount()
		}
		if hadTTL {
			stat.Stats.DecrementExpirationKeysCount()
		}
	}
}

func ResetStore() {
	cfg := globals.GetConfig()

	mainStore.reset()
	expirationContainer.reset()
	log.Info("Store has been reset")

	if cfg.Stats.Enabled {
		stat.Stats.Reset()
	}
}

func CleanExpiratedKeys(index int64) {
	expirationContainer.mu.RLock()
	bucket, ok := expirationContainer.Buckets[index]
	if !ok {
		expirationContainer.mu.RUnlock()
		return
	}
	expirationContainer.mu.RUnlock()

	bucket.mu.RLock()
	snapshot := make([]string, len(bucket.Keys))
	copy(snapshot, bucket.Keys)
	bucket.mu.RUnlock()

	for _, v := range snapshot {
		DeleteByKey(v)
	}

	expirationContainer.mu.Lock()
	delete(expirationContainer.Buckets, index)
	expirationContainer.mu.Unlock()
}

func KeyHasExpired(key string) bool {
	expirationContainer.mu.RLock()
	expTs, ok := expirationContainer.index[key]
	expirationContainer.mu.RUnlock()
	if !ok {
		return false
	}
	return time.Now().Unix() >= expTs
}

func CleanAllPastKeys() {
	expirationContainer.mu.RLock()
	bucketKeys := make([]int64, 0, len(expirationContainer.Buckets))
	for k := range expirationContainer.Buckets {
		bucketKeys = append(bucketKeys, k)
	}
	expirationContainer.mu.RUnlock()

	now := time.Now().Unix()
	for _, k := range bucketKeys {
		if k < now {
			CleanExpiratedKeys(k)
		}
	}
}

func hasTTL(key string) bool {
	expirationContainer.mu.RLock()
	_, ok := expirationContainer.index[key]
	expirationContainer.mu.RUnlock()
	return ok
}
