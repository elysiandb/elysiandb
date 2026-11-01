package log

import (
	"log/slog"
	"sync"
	"time"
)

type LevelInfo struct {
	Level      string     `json:"level"`
	Configured string     `json:"configured"`
	Until      *time.Time `json:"until"`
}

var override struct {
	mu    sync.Mutex
	until time.Time
	timer *time.Timer
}

func ConfiguredLevel() slog.Level {
	return slog.Level(configuredLevel.Load())
}

func CurrentLevel() LevelInfo {
	override.mu.Lock()
	defer override.mu.Unlock()

	info := LevelInfo{
		Level:      LevelName(levelVar.Level()),
		Configured: LevelName(ConfiguredLevel()),
	}

	if !override.until.IsZero() {
		until := override.until
		info.Until = &until
	}

	return info
}

func SetLevelFor(level slog.Level, d time.Duration) LevelInfo {
	override.mu.Lock()

	if override.timer != nil {
		override.timer.Stop()
		override.timer = nil
	}

	if level == ConfiguredLevel() || d <= 0 {
		override.until = time.Time{}
		levelVar.Set(ConfiguredLevel())
		override.mu.Unlock()

		return CurrentLevel()
	}

	levelVar.Set(level)
	override.until = time.Now().Add(d)
	override.timer = time.AfterFunc(d, restoreConfiguredLevel)
	override.mu.Unlock()

	return CurrentLevel()
}

func restoreConfiguredLevel() {
	override.mu.Lock()
	override.until = time.Time{}
	override.timer = nil
	levelVar.Set(ConfiguredLevel())
	override.mu.Unlock()

	Info("log level restored", "component", "audit", "level", LevelName(ConfiguredLevel()), "reason", "expired")
}

func cancelOverride() {
	override.mu.Lock()
	defer override.mu.Unlock()

	if override.timer != nil {
		override.timer.Stop()
		override.timer = nil
	}

	override.until = time.Time{}
}
