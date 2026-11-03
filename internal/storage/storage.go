package storage

import (
	"errors"
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/legacy"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
)

const (
	DataFile           = legacy.DataFile
	JsonDataFile       = legacy.JsonDataFile
	ExpirationDataFile = legacy.ExpirationFile
)

var (
	current    atomic.Pointer[datastore.Engine]
	lifecycle  sync.Mutex
	storageLog = log.For("storage")
	writeFails = log.NewThrottle(time.Minute)
)

type OpenMode int

const (
	ModeServer OpenMode = iota
	ModeTool
)

func Options(cfg *configuration.Config, mode OpenMode) datastore.Options {
	durability := journal.ModeBackground
	if cfg.Store.CrashRecovery.Enabled {
		durability = journal.ModeWrite
	}

	if strings.EqualFold(cfg.Store.Durability, "fsync") {
		durability = journal.ModeSync
	}

	interval := time.Duration(cfg.Store.FlushIntervalSeconds) * time.Second

	return datastore.Options{
		Folder:          cfg.Store.Folder,
		Shards:          cfg.Store.Shards,
		Durability:      durability,
		FlushInterval:   interval,
		CheckpointEvery: interval,
		MaxLogBytes:     cfg.Store.CrashRecovery.MaxLogMB * 1024 * 1024,
		CacheBytes:      ValueCacheBytes(cfg),
		IndexWorkers:    cfg.Api.Index.Workers,
		Background:      mode == ModeServer,
		LegacyRecovery:  cfg.Store.CrashRecovery.Enabled,
		OnKVExpired:     metrics.KVExpired,
		OnWriteError:    reportWriteError,
	}
}

func reportWriteError(err error) {
	metrics.RecoveryWriteFailed(err)
	if suppressed, ok := writeFails.Allow("log"); ok {
		storageLog.Error("log write failed", log.WithSuppressed([]any{"err", err}, suppressed)...)
	}
}

var ValueCacheBytes = func(cfg *configuration.Config) int64 {
	return 0
}

func Open(mode OpenMode) error {
	lifecycle.Lock()
	defer lifecycle.Unlock()

	if old := current.Load(); old != nil {
		if err := old.Close(); err != nil {
			storageLog.Error("cannot close the previous engine", "err", err)
		}
		current.Store(nil)
	}

	e, err := datastore.Open(Options(globals.GetConfig(), mode))
	if err != nil {
		return err
	}

	current.Store(e)

	return nil
}

func LoadDB() {
	if err := Open(ModeTool); err != nil {
		log.Fatal("cannot open the data folder", "component", "storage", "folder", globals.GetConfig().Store.Folder, "err", err)
	}
}

func LoadJsonDB() {
	e := current.Load()
	if e == nil || e.Folder() != globals.GetConfig().Store.Folder {
		LoadDB()
	}
}

func Engine() *datastore.Engine {
	return current.Load()
}

func Close() error {
	lifecycle.Lock()
	defer lifecycle.Unlock()

	e := current.Swap(nil)
	if e == nil {
		return nil
	}

	return e.Close()
}

var checkpointFailures atomic.Int64

func Checkpoint() error {
	e := current.Load()
	if e == nil {
		return nil
	}

	start := time.Now()
	err := e.Checkpoint()
	metrics.RecordFlush(flushTarget, time.Since(start), true, err)

	if err != nil {
		checkpointFailures.Add(1)
		if suppressed, ok := writeFails.Allow(flushTarget); ok {
			storageLog.Error("flush failed", log.WithSuppressed([]any{"target", flushTarget, "err", err}, suppressed)...)
		}

		return err
	}

	if n := checkpointFailures.Swap(0); n > 0 {
		writeFails.Reset(flushTarget)
		storageLog.Info("flush working again", "target", flushTarget, "failures", n)
	}

	return nil
}

const flushTarget = "store"

func WriteToDB() {
	_ = Checkpoint()
}

func WriteStoreDB() {
	_ = Checkpoint()
}

func WriteJsonDB() {
	_ = Checkpoint()
}

var ErrKeyNotFound = errors.New("key not found")

func GetByKey(key string) ([]byte, error) {
	e := current.Load()
	if e == nil {
		return nil, fmt.Errorf("key not found: %s", key)
	}

	if globals.IsApiKey(key) {
		if v, ok := apiKeyValue(e, key); ok {
			return v, nil
		}

		return nil, fmt.Errorf("key not found: %s", key)
	}

	v, _, ok := e.KVGet(key)
	if !ok {
		return nil, fmt.Errorf("key not found: %s", key)
	}

	return v, nil
}

var ApiKeyResolver func(e *datastore.Engine, key string) ([]byte, bool)

func apiKeyValue(e *datastore.Engine, key string) ([]byte, bool) {
	if ApiKeyResolver == nil {
		return nil, false
	}

	return ApiKeyResolver(e, key)
}

func GetByWildcardKeyWithLimit(pattern string, limit int) map[string][]byte {
	out := make(map[string][]byte)
	e := current.Load()
	if e == nil {
		return out
	}

	now := time.Now().Unix()
	expired := []string{}
	e.KVMatch(pattern, limit, func(key string) bool {
		value, exp, ok := e.KVGet(key)
		if !ok {
			return true
		}

		if exp > 0 && now >= exp {
			expired = append(expired, key)
			return true
		}

		out[key] = value

		return true
	})

	for _, key := range expired {
		DeleteByKey(key)
	}

	return out
}

func GetByWildcardKey(pattern string) map[string][]byte {
	return GetByWildcardKeyWithLimit(pattern, -1)
}

func DeleteByWildcardKey(pattern string) int {
	e := current.Load()
	if e == nil {
		return 0
	}

	keys := []string{}
	e.KVMatch(pattern, -1, func(key string) bool {
		keys = append(keys, key)
		return true
	})

	for _, key := range keys {
		e.KVDelete(key)
	}

	return len(keys)
}

func PutKeyValue(key string, value []byte) error {
	return putKeyValue(key, value, 0, true)
}

func PutKeyValueWithTTL(key string, value []byte, ttl int) error {
	expiresAt := int64(0)
	if ttl > 0 {
		expiresAt = time.Now().Unix() + int64(ttl)
	}

	return putKeyValue(key, value, expiresAt, expiresAt == 0)
}

func PutKeyValueWithExpiration(key string, value []byte, expiresAt int64) error {
	return putKeyValue(key, value, expiresAt, expiresAt <= 0)
}

func putKeyValue(key string, value []byte, expiresAt int64, keepTTL bool) error {
	if globals.IsApiKey(key) {
		return nil
	}

	e := current.Load()
	if e == nil {
		return fmt.Errorf("storage is not open")
	}

	if keepTTL {
		if exp, ok := e.KVExpiration(key); ok {
			expiresAt = exp
		}
	}

	return e.KVPut(key, value, expiresAt)
}

func DeleteByKey(key string) {
	if globals.IsApiKey(key) {
		return
	}

	if e := current.Load(); e != nil {
		e.KVDelete(key)
	}
}

func ResetStore() {
	if e := current.Load(); e != nil {
		if err := e.KVReset(); err != nil {
			storageLog.Error("key-value reset failed", "err", err)
		}
	}
}

func ClearAll() {
	e := current.Load()
	if e == nil {
		return
	}

	if err := e.Update(func(tx *datastore.Tx) error {
		tx.ClearAll()
		return nil
	}); err != nil {
		storageLog.Error("clear failed", "err", err)
	}
}

func CountKeys() (uint64, uint64) {
	e := current.Load()
	if e == nil {
		return 0, 0
	}

	keys, withTTL := e.KVCount()

	return uint64(keys), uint64(withTTL)
}

func ListKeys() []string {
	e := current.Load()
	if e == nil {
		return nil
	}

	return e.KVKeys()
}

func GetKeyExpiration(key string) (int64, bool) {
	e := current.Load()
	if e == nil {
		return 0, false
	}

	return e.KVExpiration(key)
}

func KeyHasExpired(key string) bool {
	exp, ok := GetKeyExpiration(key)
	if !ok {
		return false
	}

	return time.Now().Unix() >= exp
}

func CleanExpiratedKeys(now int64) {
	if e := current.Load(); e != nil {
		e.ExpireDue(now)
	}
}

func CleanAllPastKeys() {
	CleanExpiratedKeys(time.Now().Unix() - 1)
}

func CountJsonDocuments() uint64 {
	e := current.Load()
	if e == nil {
		return 0
	}

	total := uint64(0)
	for _, snap := range e.SnapshotAll() {
		total += uint64(snap.Len())
	}

	return total
}

func ParseDocumentKey(key string) (string, string, bool) {
	rest, ok := strings.CutPrefix(key, globals.ApiKeyPrefix)
	if !ok {
		return "", "", false
	}

	entity, id, ok := strings.Cut(rest, ":id:")
	if !ok || entity == "" {
		return "", "", false
	}

	return entity, id, true
}

func GetJsonByKeyNoCopy(key string) (map[string]any, error) {
	e := current.Load()
	if e == nil {
		return nil, fmt.Errorf("json store not initialized")
	}

	entity, id, ok := ParseDocumentKey(key)
	if !ok {
		return nil, fmt.Errorf("key not found: %s", key)
	}

	doc, ok := e.Snapshot(entity).Get(id)
	if !ok {
		return nil, fmt.Errorf("key not found: %s", key)
	}

	return doc.Decode()
}

var GetJsonByKey = GetJsonByKeyNoCopy

func PutJsonValue(key string, value map[string]any) error {
	e := current.Load()
	if e == nil {
		return fmt.Errorf("json store not initialized")
	}

	entity, id, ok := ParseDocumentKey(key)
	if !ok {
		return fmt.Errorf("invalid document key: %s", key)
	}

	return e.Update(func(tx *datastore.Tx) error {
		return tx.Put(entity, id, value)
	})
}

func DeleteJsonByKey(key string) {
	e := current.Load()
	if e == nil {
		return
	}

	entity, id, ok := ParseDocumentKey(key)
	if !ok {
		return
	}

	_ = e.Update(func(tx *datastore.Tx) error {
		tx.Delete(entity, id)
		return nil
	})
}

func ListJsonKeys() []string {
	e := current.Load()
	if e == nil {
		return nil
	}

	keys := []string{}
	for name, snap := range e.SnapshotAll() {
		snap.Ascend(0, func(d datastore.Doc) bool {
			keys = append(keys, globals.ApiSingleEntityKey(name, d.ID()))
			return true
		})
	}

	return keys
}
