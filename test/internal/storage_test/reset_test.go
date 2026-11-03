package storage_test

import (
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
)

func TestResetStoreDeletesKVKeysAndKeepsTheDocuments(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{Folder: t.TempDir(), Shards: 4},
		Stats: configuration.StatsConfig{Enabled: true},
	})
	stat.Init()
	storage.LoadDB()
	t.Cleanup(func() { _ = storage.Close() })

	api_storage.WriteEntity("books", map[string]any{"id": "b1"})
	api_storage.WriteEntity("books", map[string]any{"id": "b2"})

	indexKey := globals.ApiEntityIndexIdKey("books")
	typesKey := globals.ApiAllEntityTypesListKey()
	ttlAPIKey := globals.ApiKeyPrefix + "temporary"

	for key, value := range map[string]string{indexKey: "corrupted", typesKey: "nothing", "note": "hello"} {
		if err := storage.PutKeyValue(key, []byte(value)); err != nil {
			t.Fatal(err)
		}
	}
	if err := storage.PutKeyValueWithTTL("session", []byte("x"), 3600); err != nil {
		t.Fatal(err)
	}
	if err := storage.PutKeyValueWithTTL(ttlAPIKey, []byte("y"), 3600); err != nil {
		t.Fatal(err)
	}

	if raw, err := storage.GetByKey(indexKey); err != nil || string(raw) != "b1\nb2" {
		t.Fatalf("the id index is built from the engine and cannot be overwritten, got %q %v", raw, err)
	}

	storage.ResetStore()

	for _, key := range []string{"note", "session", ttlAPIKey} {
		if _, err := storage.GetByKey(key); err == nil {
			t.Fatalf("%s should not exist after a reset", key)
		}
	}

	if raw, err := storage.GetByKey(indexKey); err != nil || string(raw) != "b1\nb2" {
		t.Fatalf("the id index must survive a reset, got %q %v", raw, err)
	}
	if raw, err := storage.GetByKey(typesKey); err != nil || string(raw) != "books" {
		t.Fatalf("the entity types must survive a reset, got %q %v", raw, err)
	}

	if _, ok := storage.GetKeyExpiration("session"); ok {
		t.Fatalf("the expiration of a deleted key must be gone")
	}

	if err := storage.PutKeyValueWithTTL("fresh", []byte("z"), 60); err != nil {
		t.Fatal(err)
	}
	if ts, ok := storage.GetKeyExpiration("fresh"); !ok || ts < time.Now().Unix() {
		t.Fatalf("a new key gets its expiration, got %d %v", ts, ok)
	}

	keys, withTTL := storage.CountKeys()
	if keys != 1 || withTTL != 1 {
		t.Fatalf("expected 1 key with a TTL, got %d %d", keys, withTTL)
	}

	if got := api_storage.CountEntities("books"); got != 2 {
		t.Fatalf("documents are not touched by a reset, got %d", got)
	}
}

func TestListJsonKeysAndCountJsonDocuments(t *testing.T) {
	globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: t.TempDir(), Shards: 4}})
	storage.LoadDB()
	t.Cleanup(func() { _ = storage.Close() })

	for _, key := range []string{"api:entity:books:id:1", "api:entity:books:id:2"} {
		if err := storage.PutJsonValue(key, map[string]any{"id": key}); err != nil {
			t.Fatal(err)
		}
	}

	keys := storage.ListJsonKeys()
	if len(keys) != 2 || storage.CountJsonDocuments() != 2 {
		t.Fatalf("unexpected keys %v and count %d", keys, storage.CountJsonDocuments())
	}
}
