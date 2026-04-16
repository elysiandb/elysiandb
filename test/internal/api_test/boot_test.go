package api_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
)

func TestEngineDetection(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Engine: configuration.EngineConfig{
			Name: "internal",
		},
	})

	if !engine.IsEngineInternal() {
		t.Fatal("expected internal engine")
	}

	if engine.IsEngineMongoDB() {
		t.Fatal("should not be MongoDB")
	}

	globals.SetConfig(&configuration.Config{
		Engine: configuration.EngineConfig{
			Name: "mongodb",
		},
	})

	if !engine.IsEngineMongoDB() {
		t.Fatal("expected MongoDB engine")
	}

	if engine.IsEngineInternal() {
		t.Fatal("should not be internal")
	}
}

func TestDefaultEngineIsInternal(t *testing.T) {
	globals.SetConfig(&configuration.Config{})

	if !engine.IsEngineInternal() {
		t.Fatal("default engine should be internal")
	}
}
