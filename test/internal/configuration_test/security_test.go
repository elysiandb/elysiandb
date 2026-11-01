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
	cfg.Engine.Name = "mongodb"
	cfg.Engine.URI = "mongodb://elysian:s3cret@localhost:27017/elysiandb"

	out := cfg.ToJson()
	if strings.Contains(out, "test-token") || strings.Contains(out, "s3cret") {
		t.Fatalf("expected secrets to be redacted, got %s", out)
	}

	var decoded cfgpkg.Config
	if err := json.Unmarshal([]byte(out), &decoded); err != nil {
		t.Fatal(err)
	}

	if decoded.Security.Authentication.Token != cfgpkg.RedactedValue || decoded.Security.Authentication.Mode != "token" {
		t.Fatalf("unexpected authentication block %+v", decoded.Security.Authentication)
	}

	if decoded.Engine.URI != "mongodb://elysian:xxxxx@localhost:27017/elysiandb" {
		t.Fatalf("unexpected engine URI %q", decoded.Engine.URI)
	}

	if cfg.Security.Authentication.Token != "test-token" || !strings.Contains(cfg.Engine.URI, "s3cret") {
		t.Fatalf("ToJson must not change the configuration")
	}

	empty := (&cfgpkg.Config{}).ToJson()
	var decodedEmpty cfgpkg.Config
	_ = json.Unmarshal([]byte(empty), &decodedEmpty)
	if decodedEmpty.Security.Authentication.Token != "" || decodedEmpty.Engine.URI != "" {
		t.Fatalf("expected empty values to stay empty, got %s", empty)
	}

	invalid := &cfgpkg.Config{}
	invalid.Engine.URI = "mongodb://user:pa%zzss@localhost:27017/elysiandb"
	if out := invalid.ToJson(); strings.Contains(out, "pa%zzss") || !strings.Contains(out, `"URI":"***"`) {
		t.Fatalf("expected an unparsable URI to be fully redacted, got %s", out)
	}

	noPassword := &cfgpkg.Config{}
	noPassword.Engine.URI = "mongodb://localhost:27017/elysiandb"
	if !strings.Contains(noPassword.ToJson(), "mongodb://localhost:27017/elysiandb") {
		t.Fatalf("expected a URI without password to be kept, got %s", noPassword.ToJson())
	}
}

func TestConfigToJsonRedactsSecretQueryParameters(t *testing.T) {
	cfg := &cfgpkg.Config{}
	cfg.Engine.URI = "mongodb://elysian:s3cret@localhost:27017/elysiandb?authMechanismProperties=AWS_SESSION_TOKEN:abc123&tlsCertificateKeyFilePassword=k3y&replicaSet=rs0"

	out := cfg.ToJson()
	for _, secret := range []string{"s3cret", "abc123", "k3y"} {
		if strings.Contains(out, secret) {
			t.Fatalf("expected %q to be redacted, got %s", secret, out)
		}
	}

	if !strings.Contains(out, "replicaSet=rs0") {
		t.Fatalf("expected other parameters to be kept, got %s", out)
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
