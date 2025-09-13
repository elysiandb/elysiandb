package globals_test

import (
	"sync"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
)

func TestGetConfig_WhenUnset_IsNil(t *testing.T) {
	globals.SetConfig(nil)

	if got := globals.GetConfig(); got != nil {
		t.Fatalf("GetConfig() = %#v, want <nil>", got)
	}
}

func TestSetThenGetConfig_SamePointerAndValues(t *testing.T) {
	globals.SetConfig(nil)

	want := &configuration.Config{
		Store: configuration.StoreConfig{
			Folder: "tmp-folder",
			Shards: 16,
		},
		Server: configuration.ServersConfig{
			HTTP: configuration.ServerConfig{Enabled: true, Host: "127.0.0.1", Port: 9090},
			TCP:  configuration.ServerConfig{Enabled: false, Host: "0.0.0.0", Port: 8088},
		},
		Log:   configuration.LogConfig{FlushIntervalSeconds: 5},
		Stats: configuration.StatsConfig{Enabled: true},
	}

	globals.SetConfig(want)
	got := globals.GetConfig()

	if got != want {
		t.Fatalf("GetConfig() returned different pointer: got=%p want=%p", got, want)
	}
	if got.Store.Shards != 16 || got.Server.HTTP.Port != 9090 || !got.Stats.Enabled {
		t.Fatalf("unexpected values: %#v", got)
	}
}

func TestConcurrentSetAndGet_NoPanics_FinalIsKnownValue(t *testing.T) {
	globals.SetConfig(nil)

	c1 := &configuration.Config{Store: configuration.StoreConfig{Folder: "a", Shards: 4}}
	c2 := &configuration.Config{Store: configuration.StoreConfig{Folder: "b", Shards: 8}}
	c3 := &configuration.Config{Store: configuration.StoreConfig{Folder: "c", Shards: 12}}
	known := []*configuration.Config{c1, c2, c3}

	var wg sync.WaitGroup

	wg.Add(1)
	go func() {
		defer wg.Done()
		for i := 0; i < 1000; i++ {
			globals.SetConfig(c1)
			globals.SetConfig(c2)
			globals.SetConfig(c3)
		}
	}()

	readers := 4
	wg.Add(readers)
	for r := 0; r < readers; r++ {
		go func() {
			defer wg.Done()
			for i := 0; i < 2000; i++ {
				_ = globals.GetConfig()
				time.Sleep(time.Microsecond)
			}
		}()
	}

	wg.Wait()

	final := globals.GetConfig()
	if final == nil {
		t.Fatalf("final GetConfig() returned nil, want one of known configs")
	}
	isKnown := false
	for _, k := range known {
		if final == k {
			isKnown = true
			break
		}
	}
	if !isKnown {
		t.Fatalf("final config pointer %p is not one of known values %p/%p/%p", final, c1, c2, c3)
	}
}
