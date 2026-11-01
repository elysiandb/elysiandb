package recovery_test

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/recovery"
	"github.com/taymour/elysiandb/internal/storage"
)

func setupTempDir(t *testing.T) string {
	t.Helper()
	tmp := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder: tmp,
			CrashRecovery: configuration.CrashRecoveryConfig{
				Enabled:  true,
				MaxLogMB: 1,
			},
		},
	})
	return tmp
}

func TestJsonRecovery_LogReplayAndClear(t *testing.T) {
	dir := setupTempDir(t)
	recovery.ActivateJsonRecoveryLog(func() {})
	recovery.LogJsonPut("key1", map[string]interface{}{"a": 1})
	recovery.LogJsonDelete("key2")
	f := filepath.Join(dir, "elysiandb.json.recovery.log")
	if _, err := os.Stat(f); err != nil {
		t.Fatalf("expected log file: %v", err)
	}
	var puts, dels int
	recovery.ReplayJsonRecoveryLog(
		func(k string, v map[string]interface{}) error { puts++; return nil },
		func(k string) { dels++ },
	)
	if puts != 1 || dels != 1 {
		t.Fatalf("puts=%d dels=%d", puts, dels)
	}
	if _, err := os.Stat(f); !os.IsNotExist(err) {
		t.Fatalf("expected log cleared, got err=%v", err)
	}
	recovery.ClearJsonRecoveryLog()
}

func TestStoreRecovery_LogReplayAndClear(t *testing.T) {
	dir := setupTempDir(t)
	recovery.ActivateStoreRecoveryLog(func() {})
	recovery.LogStorePut("k1", []byte("v1"))
	recovery.LogStorePutWithTTL("k3", []byte("v3"), 1)
	recovery.LogStoreDelete("k2")
	f := filepath.Join(dir, "elysiandb.store.recovery.log")
	if _, err := os.Stat(f); err != nil {
		t.Fatalf("expected log file: %v", err)
	}
	time.Sleep(2 * time.Second)
	var puts, dels int
	recovery.ReplayStoreRecoveryLog(
		func(k string, v []byte, expiresAt int64) error { puts++; return nil },
		func(k string) { dels++ },
	)
	if puts < 1 || dels != 1 {
		t.Fatalf("puts=%d dels=%d", puts, dels)
	}
	if _, err := os.Stat(f); !os.IsNotExist(err) {
		t.Fatalf("expected log cleared, got err=%v", err)
	}
	recovery.ClearStoreRecoveryLog()
}

func TestJsonRecovery_InvalidJson(t *testing.T) {
	dir := setupTempDir(t)
	path := filepath.Join(dir, "elysiandb.json.recovery.log")
	_ = os.WriteFile(path, []byte("not-json\n"), 0o644)
	recovery.ReplayJsonRecoveryLog(func(k string, v map[string]interface{}) error { return nil }, func(k string) {})
}

func TestStoreRecovery_InvalidJson(t *testing.T) {
	dir := setupTempDir(t)
	path := filepath.Join(dir, "elysiandb.store.recovery.log")
	_ = os.WriteFile(path, []byte("bad\n"), 0o644)
	recovery.ReplayStoreRecoveryLog(func(k string, v []byte, expiresAt int64) error { return nil }, func(k string) {})
}

func TestJsonRecovery_LogSizeTrigger(t *testing.T) {
	setupTempDir(t)
	triggered := false
	recovery.ActivateJsonRecoveryLog(func() { triggered = true })
	bigVal := map[string]interface{}{"x": string(make([]byte, 1024*1024))}
	recovery.LogJsonPut("big", bigVal)
	if !triggered {
		t.Fatalf("expected SaveJsonDBFunc triggered")
	}
}

func TestStoreRecovery_LogSizeTrigger(t *testing.T) {
	setupTempDir(t)
	triggered := false
	recovery.ActivateStoreRecoveryLog(func() { triggered = true })
	large := make([]byte, 1024*1024)
	recovery.LogStorePut("big", large)
	if !triggered {
		t.Fatalf("expected SaveDBFunc triggered")
	}
}

func TestJsonRecovery_EmptyReplay(t *testing.T) {
	setupTempDir(t)
	recovery.ReplayJsonRecoveryLog(func(k string, v map[string]interface{}) error { return nil }, func(k string) {})
}

func TestStoreRecovery_EmptyReplay(t *testing.T) {
	setupTempDir(t)
	recovery.ReplayStoreRecoveryLog(func(k string, v []byte, expiresAt int64) error { return nil }, func(k string) {})
}

func TestJsonRecovery_MultipleOps(t *testing.T) {
	dir := setupTempDir(t)
	f := filepath.Join(dir, "elysiandb.json.recovery.log")
	type op struct {
		Op    string                 `json:"op"`
		Key   string                 `json:"key"`
		Value map[string]interface{} `json:"value,omitempty"`
	}
	ops := []op{
		{Op: "put", Key: "a", Value: map[string]interface{}{"x": 1}},
		{Op: "del", Key: "b"},
	}
	var buf []byte
	for _, o := range ops {
		b, _ := json.Marshal(o)
		buf = append(buf, b...)
		buf = append(buf, '\n')
	}
	_ = os.WriteFile(f, buf, 0o644)
	var puts, dels int
	recovery.ReplayJsonRecoveryLog(
		func(k string, v map[string]interface{}) error { puts++; return nil },
		func(k string) { dels++ },
	)
	if puts != 1 || dels != 1 {
		t.Fatalf("puts=%d dels=%d", puts, dels)
	}
}

func TestStoreRecovery_MultipleOpsWithTTL(t *testing.T) {
	dir := setupTempDir(t)
	f := filepath.Join(dir, "elysiandb.store.recovery.log")
	type op struct {
		Op    string `json:"op"`
		Key   string `json:"key"`
		Value []byte `json:"value,omitempty"`
		TTL   int    `json:"ttl,omitempty"`
	}
	expired := int(time.Now().Unix()) - 1
	valid := int(time.Now().Unix()) + 5
	ops := []op{
		{Op: "put", Key: "expired", Value: []byte("v1"), TTL: expired},
		{Op: "put", Key: "valid", Value: []byte("v2"), TTL: valid},
		{Op: "del", Key: "b"},
	}
	var buf []byte
	for _, o := range ops {
		b, _ := json.Marshal(o)
		buf = append(buf, b...)
		buf = append(buf, '\n')
	}
	_ = os.WriteFile(f, buf, 0o644)
	var puts, dels int
	recovery.ReplayStoreRecoveryLog(
		func(k string, v []byte, expiresAt int64) error { puts++; return nil },
		func(k string) { dels++ },
	)
	if puts != 1 || dels != 1 {
		t.Fatalf("puts=%d dels=%d", puts, dels)
	}
}

func TestJsonRecovery_ClearWithoutFile(t *testing.T) {
	setupTempDir(t)
	recovery.ClearJsonRecoveryLog()
}

func TestStoreRecovery_ClearWithoutFile(t *testing.T) {
	setupTempDir(t)
	recovery.ClearStoreRecoveryLog()
}

func TestJsonRecovery_ReplayWithUnreadableFile(t *testing.T) {
	dir := setupTempDir(t)
	path := filepath.Join(dir, "elysiandb.json.recovery.log")
	_ = os.WriteFile(path, []byte("{bad"), 0o000)
	recovery.ReplayJsonRecoveryLog(func(k string, v map[string]interface{}) error { return nil }, func(k string) {})
}

func TestStoreRecovery_ReplayWithUnreadableFile(t *testing.T) {
	dir := setupTempDir(t)
	path := filepath.Join(dir, "elysiandb.store.recovery.log")
	_ = os.WriteFile(path, []byte("{bad"), 0o000)
	recovery.ReplayStoreRecoveryLog(func(k string, v []byte, expiresAt int64) error { return nil }, func(k string) {})
}

func TestStoreRecovery_ReplayKeepsTheLoggedExpiration(t *testing.T) {
	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{
			Folder: dir,
			Shards: 4,
			CrashRecovery: configuration.CrashRecoveryConfig{
				Enabled:  true,
				MaxLogMB: 1,
			},
		},
	})

	expiresAt := time.Now().Unix() + 3600
	writeLog := func(file string, ops ...map[string]any) {
		var buf []byte
		for _, op := range ops {
			b, _ := json.Marshal(op)
			buf = append(append(buf, b...), '\n')
		}

		_ = os.WriteFile(filepath.Join(dir, file), buf, 0o644)
	}

	writeLog(recovery.StoreRecoveryFile,
		map[string]any{"op": "put", "key": "session", "value": []byte("short"), "ttl": expiresAt},
		map[string]any{"op": "put", "key": "plain", "value": []byte("kept")},
	)
	writeLog(recovery.RecoveryFile, map[string]any{"op": "put", "key": "doc", "value": map[string]any{"id": "doc"}})

	storage.LoadDB()
	storage.LoadJsonDB()
	recovery.ActivateStoreRecoveryLog(func() {})
	recovery.ActivateJsonRecoveryLog(func() {})

	recovery.ReplayJsonRecoveryLog(storage.PutJsonValue, storage.DeleteJsonByKey)
	recovery.ReplayStoreRecoveryLog(storage.PutKeyValueWithExpiration, storage.DeleteByKey)

	if doc, err := storage.GetJsonByKey("doc"); err != nil || doc["id"] != "doc" {
		t.Fatalf("expected the replayed document, got %v (%v)", doc, err)
	}

	if got, err := storage.GetByKey("session"); err != nil || string(got) != "short" {
		t.Fatalf("expected the replayed key, got %q (%v)", got, err)
	}

	if got, ok := storage.GetKeyExpiration("session"); !ok || got != expiresAt {
		t.Fatalf("expected the replayed key to expire at %d, got %d (%v)", expiresAt, got, ok)
	}

	if _, ok := storage.GetKeyExpiration("plain"); ok {
		t.Fatalf("a key logged without TTL must not get an expiration")
	}
}

func setupStores(t *testing.T) string {
	t.Helper()

	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: dir, Shards: 4}})
	storage.LoadDB()
	storage.LoadJsonDB()

	return dir
}

func writeRecoveryLog(t *testing.T, path string, ops []map[string]any, tail string) {
	t.Helper()

	var buf []byte
	for _, op := range ops {
		b, _ := json.Marshal(op)
		buf = append(append(buf, b...), '\n')
	}

	if err := os.WriteFile(path, append(buf, tail...), 0o644); err != nil {
		t.Fatalf("cannot write %s: %v", path, err)
	}
}

func TestStoreRecovery_ReplaysEntriesOfAnySize(t *testing.T) {
	dir := setupStores(t)
	path := filepath.Join(dir, recovery.StoreRecoveryFile)
	big := bytes.Repeat([]byte("x"), 200_000)

	writeRecoveryLog(t, path, []map[string]any{
		{"op": "put", "key": "before", "value": []byte("ok")},
		{"op": "put", "key": "big", "value": big},
		{"op": "put", "key": "after", "value": []byte("ok")},
		{"op": "put", "key": "deleted", "value": []byte("ok")},
		{"op": "del", "key": "deleted"},
	}, `{"op":"put","key":"truncated","val`)

	recovery.ReplayStoreRecoveryLog(storage.PutKeyValueWithExpiration, storage.DeleteByKey)

	for key, value := range map[string][]byte{"before": []byte("ok"), "big": big, "after": []byte("ok")} {
		if got, err := storage.GetByKey(key); err != nil || !bytes.Equal(got, value) {
			t.Fatalf("expected %s to be replayed, got %d bytes (%v)", key, len(got), err)
		}
	}

	for _, key := range []string{"deleted", "truncated"} {
		if _, err := storage.GetByKey(key); err == nil {
			t.Fatalf("%s must not be replayed", key)
		}
	}

	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatalf("expected the replayed log to be cleared, got %v", err)
	}
}

func TestJsonRecovery_ReplaysEntriesOfAnySize(t *testing.T) {
	dir := setupStores(t)
	path := filepath.Join(dir, recovery.RecoveryFile)
	big := strings.Repeat("x", 200_000)

	writeRecoveryLog(t, path, []map[string]any{
		{"op": "put", "key": "before", "value": map[string]any{"id": "before"}},
		{"op": "put", "key": "big", "value": map[string]any{"id": "big", "text": big}},
		{"op": "put", "key": "after", "value": map[string]any{"id": "after"}},
	}, "")

	recovery.ReplayJsonRecoveryLog(storage.PutJsonValue, storage.DeleteJsonByKey)

	for _, key := range []string{"before", "big", "after"} {
		if doc, err := storage.GetJsonByKey(key); err != nil || doc["id"] != key {
			t.Fatalf("expected %s to be replayed, got %v", key, err)
		}
	}

	if doc, _ := storage.GetJsonByKey("big"); doc["text"] != big {
		t.Fatalf("expected the large document to be replayed in full")
	}

	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatalf("expected the replayed log to be cleared, got %v", err)
	}
}

func TestRecovery_KeepsALogItCannotRead(t *testing.T) {
	for name, tc := range map[string]struct {
		file   string
		replay func()
	}{
		"store": {recovery.StoreRecoveryFile, func() {
			recovery.ReplayStoreRecoveryLog(func(k string, v []byte, expiresAt int64) error { return nil }, func(k string) {})
		}},
		"json": {recovery.RecoveryFile, func() {
			recovery.ReplayJsonRecoveryLog(func(k string, v map[string]any) error { return nil }, func(k string) {})
		}},
	} {
		t.Run(name, func(t *testing.T) {
			dir := setupStores(t)
			path := filepath.Join(dir, tc.file)
			if err := os.Mkdir(path, 0o755); err != nil {
				t.Fatalf("cannot create %s: %v", path, err)
			}

			tc.replay()

			if _, err := os.Stat(path); !os.IsNotExist(err) {
				t.Fatalf("expected the unreadable log to be moved, got %v", err)
			}

			if _, err := os.Stat(path + recovery.FailedReplaySuffix); err != nil {
				t.Fatalf("expected the unreadable log to be kept as %s: %v", path+recovery.FailedReplaySuffix, err)
			}
		})
	}
}
