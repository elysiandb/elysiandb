package configuration_test

import (
	"bytes"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	cfgpkg "github.com/taymour/elysiandb/internal/configuration"
)

func captureStdout(t *testing.T, fn func()) string {
	t.Helper()

	original := os.Stdout
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}

	os.Stdout = w
	fn()
	w.Close()
	os.Stdout = original

	var buf bytes.Buffer
	_, _ = io.Copy(&buf, r)

	return buf.String()
}

func TestLoadConfig_UnknownKeysAreReportedButIgnored(t *testing.T) {
	tmp := t.TempDir()
	path := filepath.Join(tmp, "elysian.yaml")
	content := []byte(`
store:
  folder: /tmp/x
  shards: 8
  json:
    arenaChunkSize: 1048576
authentication:
  enabled: true
server:
  http:
    enabled: true
    port: 8089
    cors:
      allowedOrigins: ["https://app.example", "http://localhost:5173"]
`)
	if err := os.WriteFile(path, content, 0o644); err != nil {
		t.Fatal(err)
	}

	var cfg *cfgpkg.Config
	var err error
	out := captureStdout(t, func() {
		cfg, err = cfgpkg.LoadConfig(path)
	})

	if err != nil || cfg == nil {
		t.Fatalf("expected unknown keys not to break loading, got %v", err)
	}

	if cfg.Store.Shards != 8 || cfg.Server.HTTP.Port != 8089 || cfg.Security.Authentication.Enabled {
		t.Fatalf("unexpected config %+v", cfg)
	}

	if got := cfg.Server.HTTP.CORS.AllowedOrigins; len(got) != 2 || got[0] != "https://app.example" {
		t.Fatalf("expected the CORS allow-list to be parsed, got %v", got)
	}

	for _, want := range []string{"WARN", "field json not found", "field authentication not found"} {
		if !strings.Contains(out, want) {
			t.Fatalf("expected the warning to mention %q, got %q", want, out)
		}
	}
}

func TestLoadConfig_KnownKeysProduceNoWarning(t *testing.T) {
	tmp := t.TempDir()
	path := filepath.Join(tmp, "elysian.yaml")
	content := []byte(`
store:
  folder: /tmp/x
security:
  authentication:
    enabled: true
    mode: user
`)
	if err := os.WriteFile(path, content, 0o644); err != nil {
		t.Fatal(err)
	}

	out := captureStdout(t, func() {
		if _, err := cfgpkg.LoadConfig(path); err != nil {
			t.Errorf("LoadConfig: %v", err)
		}
	})

	if strings.Contains(out, "WARN") {
		t.Fatalf("expected no warning, got %q", out)
	}
}

func TestConfigToJsonRedactsSecrets(t *testing.T) {
	cfg := &cfgpkg.Config{}
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "token"
	cfg.Security.Authentication.Token = "test-token"
	cfg.Monitoring.Prometheus.Token = "prom-token"
	cfg.Engine.Name = "internal"

	out := cfg.ToJson()
	if strings.Contains(out, "test-token") || strings.Contains(out, "prom-token") {
		t.Fatalf("expected secrets to be redacted, got %s", out)
	}

	var decoded cfgpkg.Config
	if err := json.Unmarshal([]byte(out), &decoded); err != nil {
		t.Fatal(err)
	}

	if decoded.Security.Authentication.Token != cfgpkg.RedactedValue || decoded.Security.Authentication.Mode != "token" {
		t.Fatalf("unexpected authentication block %+v", decoded.Security.Authentication)
	}

	if decoded.Monitoring.Prometheus.Token != cfgpkg.RedactedValue || decoded.Engine.Name != "internal" {
		t.Fatalf("unexpected decoded configuration %+v", decoded)
	}

	if cfg.Security.Authentication.Token != "test-token" || cfg.Monitoring.Prometheus.Token != "prom-token" {
		t.Fatalf("ToJson must not change the configuration")
	}

	empty := (&cfgpkg.Config{}).ToJson()
	var decodedEmpty cfgpkg.Config
	_ = json.Unmarshal([]byte(empty), &decodedEmpty)
	if decodedEmpty.Security.Authentication.Token != "" || decodedEmpty.Monitoring.Prometheus.Token != "" {
		t.Fatalf("expected empty values to stay empty, got %s", empty)
	}
}

func TestLoadConfigChecksTheEngine(t *testing.T) {
	cases := map[string]struct {
		block   string
		wantErr bool
		warns   bool
	}{
		"no engine block":  {block: ""},
		"empty name":       {block: "engine:\n  name: \"\"\n"},
		"internal":         {block: "engine:\n  name: internal\n"},
		"former uri key":   {block: "engine:\n  name: internal\n  uri: \"db://user:secret@localhost:1/x\"\n", warns: true},
		"other engine":     {block: "engine:\n  name: other\n", wantErr: true},
		"wrong case":       {block: "engine:\n  name: Internal\n", wantErr: true},
		"surrounding text": {block: "engine:\n  name: \" internal\"\n", wantErr: true},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "elysian.yaml")
			content := "store:\n  folder: /tmp/x\n" + tc.block
			if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}

			var cfg *cfgpkg.Config
			var err error
			out := captureStdout(t, func() { cfg, err = cfgpkg.LoadConfig(path) })

			if tc.wantErr {
				if err == nil || !strings.Contains(err.Error(), "engine.name") {
					t.Fatalf("expected an engine.name error, got %v", err)
				}
				return
			}

			if err != nil || cfg == nil {
				t.Fatalf("unexpected error %v", err)
			}

			if strings.Contains(out, "WARN") != tc.warns {
				t.Fatalf("unexpected warning output %q", out)
			}

			if strings.Contains(out, "secret") || strings.Contains(cfg.ToJson(), "secret") {
				t.Fatalf("the former uri key must not be kept, got %q and %s", out, cfg.ToJson())
			}
		})
	}
}

func TestLoadConfigChecksTheAuthenticationMode(t *testing.T) {
	cases := map[string]struct {
		block   string
		mode    string
		wantErr bool
	}{
		"unknown mode": {block: "enabled: true\n    mode: tokne\n    token: abc", wantErr: true},
		"missing mode": {block: "enabled: true", wantErr: true},
		"mixed case":   {block: "enabled: true\n    mode: \" User \"", mode: "user"},
		"disabled":     {block: "enabled: false\n    mode: anything", mode: "anything"},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "elysian.yaml")
			content := "store:\n  folder: /tmp/x\nsecurity:\n  authentication:\n    " + tc.block + "\n"
			if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}

			var cfg *cfgpkg.Config
			var err error
			captureStdout(t, func() { cfg, err = cfgpkg.LoadConfig(path) })

			if tc.wantErr {
				if err == nil {
					t.Fatalf("expected an error")
				}
				return
			}

			if err != nil || cfg.Security.Authentication.Mode != tc.mode {
				t.Fatalf("unexpected result %v %v", cfg, err)
			}
		})
	}
}
