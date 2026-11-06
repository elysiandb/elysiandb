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
	Level                string `yaml:"level"`
	Format               string `yaml:"format"`
	AccessLog            bool   `yaml:"accessLog"`
	SlowRequestMs        int    `yaml:"slowRequestMs"`
	BufferLines          int    `yaml:"bufferLines"`
	FlushIntervalSeconds int    `yaml:"flushIntervalSeconds"`
}

type PrometheusConfig struct {
	Enabled bool   `yaml:"enabled"`
	Token   string `yaml:"token"`
}

type MonitoringConfig struct {
	Prometheus PrometheusConfig `yaml:"prometheus"`
	Pprof      PprofConfig      `yaml:"pprof"`
}

type PprofConfig struct {
	Enabled bool `yaml:"enabled"`
}

type ServerConfig struct {
	Enabled             bool       `yaml:"enabled"`
	Host                string     `yaml:"host"`
	Port                int        `yaml:"port"`
	CORS                CORSConfig `yaml:"cors"`
	TrustedProxies      []string   `yaml:"trustedProxies"`
	ReadTimeoutSeconds  int        `yaml:"readTimeoutSeconds"`
	WriteTimeoutSeconds int        `yaml:"writeTimeoutSeconds"`
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
	Durability           string              `yaml:"durability"`
	Memory               StoreMemoryConfig   `yaml:"memory"`
	MinFreeDiskMB        int64               `yaml:"minFreeDiskMB"`
	Backup               StoreBackupConfig   `yaml:"backup"`
}

type StoreBackupConfig struct {
	Folder       string `yaml:"folder"`
	EveryMinutes int    `yaml:"everyMinutes"`
	Keep         int    `yaml:"keep"`
}

type StoreMemoryConfig struct {
	LimitMB int64 `yaml:"limitMB"`
}

type StatsConfig struct {
	Enabled bool `yaml:"enabled"`
}

type ApiSchemaConfig struct {
	Enabled bool `yaml:"enabled"`
	Strict  bool `yaml:"strict"`
}

type ApiConfig struct {
	Index    ApiIndexConfig    `yaml:"index"`
	Cache    ApiCacheConfig    `yaml:"cache"`
	Schema   ApiSchemaConfig   `yaml:"schema"`
	Hooks    HooksConfig       `yaml:"hooks"`
	Query    ApiQueryConfig    `yaml:"query"`
	Response ApiResponseConfig `yaml:"response"`
}

type ApiQueryConfig struct {
	Workers            int   `yaml:"workers"`
	TimeoutMs          int   `yaml:"timeoutMs"`
	MaxQueued          int   `yaml:"maxQueued"`
	QueueTimeoutMs     int   `yaml:"queueTimeoutMs"`
	CancelOnDisconnect *bool `yaml:"cancelOnDisconnect"`
}

func (c ApiQueryConfig) CancelsAbandonedRequests() bool {
	return c.CancelOnDisconnect == nil || *c.CancelOnDisconnect
}

type ApiResponseConfig struct {
	SpoolMB int `yaml:"spoolMB"`
}

type HooksConfig struct {
	Enabled         bool `yaml:"enabled"`
	TimeoutMs       int  `yaml:"timeoutMs"`
	RequestBudgetMs int  `yaml:"requestBudgetMs"`
}

type ApiIndexConfig struct {
	Workers int `yaml:"workers"`
}

type ApiCacheConfig struct {
	Enabled                bool `yaml:"enabled"`
	CleanupIntervalSeconds int  `yaml:"cleanupIntervalSeconds"`
	MaxMB                  int  `yaml:"maxMB"`
	MaxEntryKB             int  `yaml:"maxEntryKB"`
}

type EngineConfig struct {
	Name string `yaml:"name"`
	URI  string `yaml:"uri"`
}

type Config struct {
	Store      StoreConfig      `yaml:"store"`
	Server     ServersConfig    `yaml:"server"`
	Log        LogConfig        `yaml:"log"`
	Security   SecurityConfig   `yaml:"security"`
	Stats      StatsConfig      `yaml:"stats"`
	Api        ApiConfig        `yaml:"api"`
	AdminUI    AdminUIConfig    `yaml:"adminui"`
	Engine     EngineConfig     `yaml:"engine"`
	Monitoring MonitoringConfig `yaml:"monitoring"`
}

const (
	DefaultSlowRequestMs = 500
	DefaultBufferLines   = 5000
)

func DefaultConfig() Config {
	return Config{
		Log: LogConfig{
			Level:         "info",
			Format:        "text",
			SlowRequestMs: DefaultSlowRequestMs,
			BufferLines:   DefaultBufferLines,
		},
	}
}

const RedactedValue = "***"

func (c *Config) ToJson() string {
	copyConfig := *c
	if copyConfig.Security.Authentication.Token != "" {
		copyConfig.Security.Authentication.Token = RedactedValue
	}

	if copyConfig.Monitoring.Prometheus.Token != "" {
		copyConfig.Monitoring.Prometheus.Token = RedactedValue
	}

	copyConfig.Engine.URI = redactURIPassword(copyConfig.Engine.URI)

	data, err := json.Marshal(copyConfig)
	if err != nil {
		return "{}"
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
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("cannot read the configuration file: %w", err)
	}

	cfg := DefaultConfig()
	if err := yaml.Unmarshal(data, &cfg); err != nil {
		return nil, fmt.Errorf("cannot parse the configuration file: %w", err)
	}

	if err := yaml.UnmarshalStrict(data, &Config{}); err != nil {
		log.Warn("unknown configuration keys are ignored", "component", "boot", "path", path, "err", err)
	}

	if _, err := log.ParseLevel(cfg.Log.Level); err != nil {
		return nil, err
	}

	switch strings.ToLower(strings.TrimSpace(cfg.Log.Format)) {
	case "", log.FormatText, log.FormatJSON:
	default:
		return nil, fmt.Errorf("unknown log format %q: use text or json", cfg.Log.Format)
	}

	if cfg.Log.SlowRequestMs < 0 {
		return nil, fmt.Errorf("log.slowRequestMs must be 0 or more")
	}

	if cfg.Log.BufferLines < 0 {
		return nil, fmt.Errorf("log.bufferLines must be 0 or more")
	}

	switch strings.ToLower(strings.TrimSpace(cfg.Store.Durability)) {
	case "", "default", "fsync":
	default:
		return nil, fmt.Errorf("unknown store.durability %q: use default or fsync", cfg.Store.Durability)
	}

	for name, v := range map[string]int{
		"server.http.readTimeoutSeconds":  cfg.Server.HTTP.ReadTimeoutSeconds,
		"server.http.writeTimeoutSeconds": cfg.Server.HTTP.WriteTimeoutSeconds,
		"api.query.workers":               cfg.Api.Query.Workers,
		"api.query.timeoutMs":             cfg.Api.Query.TimeoutMs,
		"api.hooks.timeoutMs":             cfg.Api.Hooks.TimeoutMs,
		"api.hooks.requestBudgetMs":       cfg.Api.Hooks.RequestBudgetMs,
		"api.cache.maxMB":                 cfg.Api.Cache.MaxMB,
		"api.cache.maxEntryKB":            cfg.Api.Cache.MaxEntryKB,
		"api.response.spoolMB":            cfg.Api.Response.SpoolMB,
	} {
		if v < 0 {
			return nil, fmt.Errorf("%s must be 0 or more", name)
		}
	}

	if cfg.Store.Memory.LimitMB < 0 {
		return nil, fmt.Errorf("store.memory.limitMB must be 0 or more")
	}

	for name, v := range map[string]int{
		"api.query.maxQueued":      cfg.Api.Query.MaxQueued,
		"api.query.queueTimeoutMs": cfg.Api.Query.QueueTimeoutMs,
	} {
		if v < -1 {
			return nil, fmt.Errorf("%s must be -1 (no limit), 0 (default) or more", name)
		}
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
