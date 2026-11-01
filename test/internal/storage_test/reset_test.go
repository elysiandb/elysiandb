package storage_test

import (
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
)

func TestResetStoreDeletesKVKeysAndKeepsRESTAPIKeys(t *testing.T) {
	globals.SetConfig(&configuration.Config{
		Store: configuration.StoreConfig{Folder: t.TempDir(), Shards: 4},
		Stats: configuration.StatsConfig{Enabled: true},
	})
	stat.Init()
	storage.LoadDB()
	storage.LoadJsonDB()

	indexKey := globals.ApiEntityIndexIdKey("books")
	typesKey := globals.ApiAllEntityTypesListKey()
	ttlAPIKey := globals.ApiKeyPrefix + "temporary"

	for key, value := range map[string]string{indexKey: "b1\nb2", typesKey: "books", "note": "hello"} {
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

	storage.ResetStore()

	for _, key := range []string{"note", "session"} {
		if _, err := storage.GetByKey(key); err == nil {
			t.Fatalf("%s should be deleted by a reset", key)
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
	if ts, ok := storage.GetKeyExpiration(ttlAPIKey); !ok || ts < time.Now().Unix() {
		t.Fatalf("a kept key keeps its expiration, got %d %v", ts, ok)
	}

	keys, withTTL := storage.CountKeys()
	if keys != 3 || withTTL != 1 {
		t.Fatalf("expected 3 keys and 1 with a TTL, got %d %d", keys, withTTL)
	}
}

func TestListJsonKeysAndCountJsonDocuments(t *testing.T) {
	globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: t.TempDir(), Shards: 4}})
	storage.LoadDB()
	storage.LoadJsonDB()

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
