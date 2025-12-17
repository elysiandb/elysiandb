package configuration_test

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	cfgpkg "github.com/taymour/elysiandb/internal/configuration"
)

func runAsSubprocess(t *testing.T, mode, arg string) (exitCode int, err error) {
	t.Helper()

	cmd := exec.Command(os.Args[0], "-test.run", "^TestConfigHelper$")
	cmd.Env = append(os.Environ(),
		"TEST_CFG_MODE="+mode,
		"TEST_CFG_ARG="+arg,
	)
	err = cmd.Run()
	if err == nil {
		return 0, nil
	}
	if ee, ok := err.(*exec.ExitError); ok {
		return ee.ExitCode(), nil
	}
	return -1, err
}

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

func TestLoadConfig_FileMissing_FatalExit(t *testing.T) {
	code, err := runAsSubprocess(t, "missing", "this-file-does-not-exist.yaml")
	if err != nil {
		t.Fatalf("subprocess error: %v", err)
	}
	if code == 0 {
		t.Fatalf("expected non-zero exit code when file is missing, got 0")
	}
}

func TestLoadConfig_InvalidYAML_FatalExit(t *testing.T) {
	tmp := t.TempDir()
	path := filepath.Join(tmp, "bad.yaml")
	if err := os.WriteFile(path, []byte("store: : : nope"), 0o644); err != nil {
		t.Fatalf("write bad yaml: %v", err)
	}

	code, err := runAsSubprocess(t, "badyaml", path)
	if err != nil {
		t.Fatalf("subprocess error: %v", err)
	}
	if code == 0 {
		t.Fatalf("expected non-zero exit code for invalid YAML, got 0")
	}
}

func TestConfigHelper(t *testing.T) {
	mode := os.Getenv("TEST_CFG_MODE")
	if mode == "" {
		t.Skip("helper not invoked")
	}
	arg := os.Getenv("TEST_CFG_ARG")

	switch mode {
	case "missing":
		_, _ = cfgpkg.LoadConfig(arg)
	case "badyaml":
		_, _ = cfgpkg.LoadConfig(arg)
	default:
		t.Fatalf("unknown TEST_CFG_MODE: %q", mode)
	}
}
