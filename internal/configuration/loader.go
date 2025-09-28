package configuration

import (
	"fmt"
	"os"

	"github.com/taymour/elysiandb/internal/log"
	"gopkg.in/yaml.v2"
)

type Config struct {
	Store    StoreConfig    `yaml:"store"`
	Server   ServersConfig  `yaml:"server"`
	Log      LogConfig      `yaml:"log"`
	Stats    StatsConfig    `yaml:"stats"`
	ApiCache ApiCacheConfig `yaml:"apiCache"`
}

type ServersConfig struct {
	HTTP ServerConfig `yaml:"http"`
	TCP  ServerConfig `yaml:"tcp"`
}

type LogConfig struct {
	FlushIntervalSeconds int `yaml:"flushIntervalSeconds"`
}

type ServerConfig struct {
	Enabled bool   `yaml:"enabled"`
	Host    string `yaml:"host"`
	Port    int    `yaml:"port"`
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

type ApiCacheConfig struct {
	Enabled                bool `yaml:"enabled"`
	CleanupIntervalSeconds int  `yaml:"cleanupIntervalSeconds"`
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

	fmt.Printf("Loaded config: %+v\n", cfg)

	return &cfg, nil
}
