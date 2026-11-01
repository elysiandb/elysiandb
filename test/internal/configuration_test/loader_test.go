package configuration_test

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	cfgpkg "github.com/taymour/elysiandb/internal/configuration"
)

func TestLoadConfig_OK(t *testing.T) {
	tmp := t.TempDir()

	yaml := []byte(`
store:
  folder: "` + filepath.ToSlash(tmp) + `"
  shards: 16
  flushIntervalSeconds: 7
server:
  http:
    enabled: true
    host: "127.0.0.1"
    port: 9090
  tcp:
    enabled: true
    host: "0.0.0.0"
    port: 8088
log:
  flushIntervalSeconds: 3
`)

	path := filepath.Join(tmp, "elysian.yaml")
	if err := os.WriteFile(path, yaml, 0o644); err != nil {
		t.Fatalf("write yaml: %v", err)
	}

	cfg, err := cfgpkg.LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig returned error: %v", err)
	}
	if cfg == nil {
		t.Fatalf("LoadConfig returned nil config")
	}

	if cfg.Store.Folder != filepath.ToSlash(tmp) {
		t.Errorf("Store.Folder = %q, want %q", cfg.Store.Folder, tmp)
	}
	if cfg.Store.Shards != 16 {
		t.Errorf("Store.Shards = %d, want 16", cfg.Store.Shards)
	}
	if cfg.Store.FlushIntervalSeconds != 7 {
		t.Errorf("Store.FlushIntervalSeconds = %d, want 7", cfg.Store.FlushIntervalSeconds)
	}

	if !cfg.Server.HTTP.Enabled || cfg.Server.HTTP.Host != "127.0.0.1" || cfg.Server.HTTP.Port != 9090 {
		t.Errorf("HTTP server parsed wrong: %+v", cfg.Server.HTTP)
	}
	if !cfg.Server.TCP.Enabled || cfg.Server.TCP.Host != "0.0.0.0" || cfg.Server.TCP.Port != 8088 {
		t.Errorf("TCP server parsed wrong: %+v", cfg.Server.TCP)
	}

	if cfg.Log.FlushIntervalSeconds != 3 {
		t.Errorf("Log.FlushIntervalSeconds = %d, want 3", cfg.Log.FlushIntervalSeconds)
	}
}

func TestLoadConfig_FileMissing_ReturnsError(t *testing.T) {
	cfg, err := cfgpkg.LoadConfig(filepath.Join(t.TempDir(), "this-file-does-not-exist.yaml"))
	if err == nil || cfg != nil {
		t.Fatalf("expected an error for a missing file, got %v %v", cfg, err)
	}
}

func TestLoadConfig_InvalidYAML_ReturnsError(t *testing.T) {
	path := filepath.Join(t.TempDir(), "bad.yaml")
	if err := os.WriteFile(path, []byte("store: [unclosed"), 0o644); err != nil {
		t.Fatalf("write bad yaml: %v", err)
	}

	cfg, err := cfgpkg.LoadConfig(path)
	if err == nil || cfg != nil {
		t.Fatalf("expected an error for invalid YAML, got %v %v", cfg, err)
	}
}

func TestLoadConfig_LogDefaults(t *testing.T) {
	path := filepath.Join(t.TempDir(), "elysian.yaml")
	if err := os.WriteFile(path, []byte("store:\n  folder: /tmp/x\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	cfg, err := cfgpkg.LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}

	if cfg.Log.Level != "info" || cfg.Log.Format != "text" || cfg.Log.AccessLog {
		t.Fatalf("unexpected log defaults %+v", cfg.Log)
	}
	if cfg.Log.SlowRequestMs != cfgpkg.DefaultSlowRequestMs || cfg.Log.BufferLines != cfgpkg.DefaultBufferLines {
		t.Fatalf("unexpected log defaults %+v", cfg.Log)
	}
	if cfg.Monitoring.Prometheus.Enabled {
		t.Fatalf("prometheus must be off by default")
	}
}

func TestLoadConfig_LogValues(t *testing.T) {
	path := filepath.Join(t.TempDir(), "elysian.yaml")
	content := "log:\n  level: debug\n  format: json\n  accessLog: true\n  slowRequestMs: 0\n  bufferLines: 0\nmonitoring:\n  prometheus:\n    enabled: true\n    token: secret\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	cfg, err := cfgpkg.LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}

	if cfg.Log.Level != "debug" || cfg.Log.Format != "json" || !cfg.Log.AccessLog || cfg.Log.SlowRequestMs != 0 || cfg.Log.BufferLines != 0 {
		t.Fatalf("unexpected log config %+v", cfg.Log)
	}
	if !cfg.Monitoring.Prometheus.Enabled || cfg.Monitoring.Prometheus.Token != "secret" {
		t.Fatalf("unexpected monitoring config %+v", cfg.Monitoring)
	}
	if strings.Contains(cfg.ToJson(), "secret") {
		t.Fatalf("the prometheus token must be redacted: %s", cfg.ToJson())
	}
}

func TestLoadConfig_RejectsInvalidLogValues(t *testing.T) {
	cases := map[string]string{
		"level":         "log:\n  level: verbose\n",
		"format":        "log:\n  format: xml\n",
		"slowRequestMs": "log:\n  slowRequestMs: -1\n",
		"bufferLines":   "log:\n  bufferLines: -5\n",
	}

	for name, content := range cases {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "elysian.yaml")
			if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
				t.Fatal(err)
			}

			if _, err := cfgpkg.LoadConfig(path); err == nil {
				t.Fatalf("expected an error")
			}
		})
	}
}
