package configuration

import (
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"strings"

	"github.com/taymour/elysiandb/internal/log"
	"gopkg.in/yaml.v2"
)

type SecurityConfig struct {
	Authentication AuthenticationConfig `yaml:"authentication"`
}

type AuthenticationConfig struct {
	Enabled bool   `yaml:"enabled"`
	Mode    string `yaml:"mode"`
	Token   string `yaml:"token"`
}

type AdminUIConfig struct {
	Enabled bool `yaml:"enabled"`
}

type ServersConfig struct {
	HTTP ServerConfig `yaml:"http"`
	TCP  ServerConfig `yaml:"tcp"`
}

type LogConfig struct {
	FlushIntervalSeconds int `yaml:"flushIntervalSeconds"`
}

type ServerConfig struct {
	Enabled        bool       `yaml:"enabled"`
	Host           string     `yaml:"host"`
	Port           int        `yaml:"port"`
	CORS           CORSConfig `yaml:"cors"`
	TrustedProxies []string   `yaml:"trustedProxies"`
}

type CORSConfig struct {
	AllowedOrigins []string `yaml:"allowedOrigins"`
}

type CrashRecoveryConfig struct {
	Enabled  bool  `yaml:"enabled"`
	MaxLogMB int64 `yaml:"maxLogMB"`
}

type StoreConfig struct {
	Folder               string              `yaml:"folder"`
	Shards               int                 `yaml:"shards"`
	FlushIntervalSeconds int                 `yaml:"flushIntervalSeconds"`
	CrashRecovery        CrashRecoveryConfig `yaml:"crashRecovery"`
}

type StatsConfig struct {
	Enabled bool `yaml:"enabled"`
}

type ApiSchemaConfig struct {
	Enabled bool `yaml:"enabled"`
	Strict  bool `yaml:"strict"`
}

type ApiConfig struct {
	Index  ApiIndexConfig  `yaml:"index"`
	Cache  ApiCacheConfig  `yaml:"cache"`
	Schema ApiSchemaConfig `yaml:"schema"`
	Hooks  HooksConfig     `yaml:"hooks"`
}

type HooksConfig struct {
	Enabled bool `yaml:"enabled"`
}

type ApiIndexConfig struct {
	Workers int `yaml:"workers"`
}

type ApiCacheConfig struct {
	Enabled                bool `yaml:"enabled"`
	CleanupIntervalSeconds int  `yaml:"cleanupIntervalSeconds"`
}

type EngineConfig struct {
	Name string `yaml:"name"`
	URI  string `yaml:"uri"`
}

type Config struct {
	Store    StoreConfig    `yaml:"store"`
	Server   ServersConfig  `yaml:"server"`
	Log      LogConfig      `yaml:"log"`
	Security SecurityConfig `yaml:"security"`
	Stats    StatsConfig    `yaml:"stats"`
	Api      ApiConfig      `yaml:"api"`
	AdminUI  AdminUIConfig  `yaml:"adminui"`
	Engine   EngineConfig   `yaml:"engine"`
}

const RedactedValue = "***"

func (c *Config) ToJson() string {
	copyConfig := *c
	if copyConfig.Security.Authentication.Token != "" {
		copyConfig.Security.Authentication.Token = RedactedValue
	}

	copyConfig.Engine.URI = redactURIPassword(copyConfig.Engine.URI)

	data, err := json.Marshal(copyConfig)
	if err != nil {
		log.Fatal("error:", err)
		return ""
	}

	return string(data)
}

func redactURIPassword(uri string) string {
	if uri == "" {
		return uri
	}

	parsed, err := url.Parse(uri)
	if err != nil {
		return RedactedValue
	}

	query := parsed.Query()
	redacted := false
	for key := range query {
		if isSecretParameter(key) {
			query.Set(key, "xxxxx")
			redacted = true
		}
	}

	if redacted {
		parsed.RawQuery = query.Encode()
	}

	return parsed.Redacted()
}

var secretParameterHints = []string{"password", "secret", "token", "key", "credential", "authmechanismproperties"}

func isSecretParameter(name string) bool {
	lower := strings.ToLower(name)
	for _, hint := range secretParameterHints {
		if strings.Contains(lower, hint) {
			return true
		}
	}

	return false
}

func LoadConfig(path string) (*Config, error) {
	fmt.Println("Loading config from", path)
	data, err := os.ReadFile(path)
	if err != nil {
		log.Fatal("error:", err)
		return nil, err
	}

	var cfg Config
	if err := yaml.Unmarshal(data, &cfg); err != nil {
		log.Fatal("error:", err)
		return nil, err
	}

	if err := yaml.UnmarshalStrict(data, &Config{}); err != nil {
		log.DirectWarn("Some configuration keys are not recognized and are ignored: ", err)
	}

	if cfg.Security.Authentication.Enabled {
		cfg.Security.Authentication.Mode = strings.ToLower(strings.TrimSpace(cfg.Security.Authentication.Mode))

		switch cfg.Security.Authentication.Mode {
		case "basic", "user":
		case "token":
			if cfg.Security.Authentication.Token == "" {
				return nil, fmt.Errorf("token authentication is enabled but no token is provided in the configuration")
			}
		default:
			return nil, fmt.Errorf("authentication is enabled with an unknown mode %q: use basic, token or user", cfg.Security.Authentication.Mode)
		}
	}

	return &cfg, nil
}
