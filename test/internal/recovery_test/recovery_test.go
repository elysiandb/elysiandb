package recovery_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/recovery"
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
		func(k string, v map[string]interface{}) error {
			puts++
			return nil
		},
		func(k string) {
			dels++
		},
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
	recovery.LogStoreDelete("k2")

	f := filepath.Join(dir, "elysiandb.store.recovery.log")
	if _, err := os.Stat(f); err != nil {
		t.Fatalf("expected log file: %v", err)
	}

	var puts, dels int
	recovery.ReplayStoreRecoveryLog(
		func(k string, v []byte) error {
			puts++
			return nil
		},
		func(k string) {
			dels++
		},
	)

	if puts != 1 || dels != 1 {
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
	recovery.ReplayStoreRecoveryLog(func(k string, v []byte) error { return nil }, func(k string) {})
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
	recovery.ReplayStoreRecoveryLog(func(k string, v []byte) error { return nil }, func(k string) {})
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
		func(k string, v map[string]interface{}) error {
			puts++
			return nil
		},
		func(k string) {
			dels++
		},
	)
	if puts != 1 || dels != 1 {
		t.Fatalf("puts=%d dels=%d", puts, dels)
	}
}

func TestStoreRecovery_MultipleOps(t *testing.T) {
	dir := setupTempDir(t)
	f := filepath.Join(dir, "elysiandb.store.recovery.log")
	type op struct {
		Op    string `json:"op"`
		Key   string `json:"key"`
		Value []byte `json:"value,omitempty"`
	}
	ops := []op{
		{Op: "put", Key: "a", Value: []byte("v")},
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
		func(k string, v []byte) error {
			puts++
			return nil
		},
		func(k string) {
			dels++
		},
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
	recovery.ReplayStoreRecoveryLog(func(k string, v []byte) error { return nil }, func(k string) {})
}
