package legacy_test

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/legacy"
	"github.com/taymour/elysiandb/internal/storage"
)

func writeFile(t *testing.T, dir, name string, data []byte) string {
	t.Helper()

	p := filepath.Join(dir, name)
	if err := os.WriteFile(p, data, 0o644); err != nil {
		t.Fatalf("write %s: %v", name, err)
	}

	return p
}

func TestReadKVFile(t *testing.T) {
	dir := t.TempDir()
	path := writeFile(t, dir, legacy.DataFile, []byte(`{"foo":"YmFy","bin":"AQID"}`))

	got, err := legacy.ReadKVFile(path)
	if err != nil {
		t.Fatal(err)
	}

	want := map[string][]byte{"foo": []byte("bar"), "bin": {0x01, 0x02, 0x03}}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got=%v want=%v", got, want)
	}

	if got, err := legacy.ReadKVFile(writeFile(t, dir, "empty.json", nil)); err != nil || len(got) != 0 {
		t.Fatalf("an empty file is an empty map, got %v %v", got, err)
	}

	if _, err := legacy.ReadKVFile(filepath.Join(dir, "missing.json")); err == nil {
		t.Fatal("expected an error for a missing file")
	}

	if _, err := legacy.ReadKVFile(writeFile(t, dir, "bad.json", []byte(`{"foo":`))); err == nil {
		t.Fatal("expected a JSON error")
	}
}

func TestReadExpirationsFile(t *testing.T) {
	dir := t.TempDir()
	path := writeFile(t, dir, legacy.ExpirationFile, []byte(`{"1690000000":["a","b"],"1690000002":[],"not-a-number":["x"]}`))

	got, err := legacy.ReadExpirationsFile(path)
	if err != nil {
		t.Fatal(err)
	}

	want := map[int64][]string{1690000000: {"a", "b"}, 1690000002: nil}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got=%#v want=%#v", got, want)
	}

	if _, err := legacy.ReadExpirationsFile(writeFile(t, dir, "bad.json", []byte(`{"169": ["ok"],`))); err == nil {
		t.Fatal("expected a JSON error")
	}
}

func logLines(ops ...map[string]any) []byte {
	var buf []byte
	for _, op := range ops {
		b, _ := json.Marshal(op)
		buf = append(append(buf, b...), '\n')
	}

	return buf
}

func TestReadReplaysTheRecoveryLogsLike024(t *testing.T) {
	dir := t.TempDir()
	now := time.Now()
	expiresAt := now.Unix() + 3600
	big := bytes.Repeat([]byte("x"), 200_000)

	kv := map[string][]byte{
		"old":                       []byte("before"),
		"gone":                      []byte("expired"),
		legacy.TypesListKey:         []byte("books"),
		legacy.IDIndexKey("books"):  []byte("b2\nghost\nb1\nb2"),
		legacy.ApiKeyPrefix + "tmp": []byte("internal"),
	}
	data, _ := json.Marshal(kv)
	writeFile(t, dir, legacy.DataFile, data)
	writeFile(t, dir, legacy.ExpirationFile, []byte(`{"1":["gone"]}`))

	docs, _ := json.Marshal(map[string]map[string]any{
		legacy.DocumentKey("books", "b1"):   {"id": "b1"},
		legacy.DocumentKey("books", "b2"):   {"id": "b2"},
		legacy.DocumentKey("authors", "a1"): {"id": "a1"},
	})
	writeFile(t, dir, legacy.JsonDataFile, docs)

	writeFile(t, dir, legacy.StoreRecoveryFile, append(logLines(
		map[string]any{"op": "put", "key": "session", "value": []byte("short"), "ttl": expiresAt},
		map[string]any{"op": "put", "key": "late", "value": []byte("x"), "ttl": now.Unix() - 10},
		map[string]any{"op": "put", "key": "big", "value": big},
		map[string]any{"op": "del", "key": "old"},
	), []byte("{not json\n")...))
	writeFile(t, dir, legacy.JsonRecoveryFile, logLines(
		map[string]any{"op": "put", "key": legacy.DocumentKey("books", "b3"), "value": map[string]any{"id": "b3", "text": string(big)}},
		map[string]any{"op": "del", "key": legacy.DocumentKey("books", "b1")},
	))

	got, err := legacy.Read(dir, true, now)
	if err != nil {
		t.Fatal(err)
	}

	if got.Replayed != 5 {
		t.Fatalf("expected 5 replayed entries, got %d", got.Replayed)
	}

	if _, ok := got.KV["old"]; ok {
		t.Fatalf("a logged delete removes the key")
	}
	if _, ok := got.KV["gone"]; ok {
		t.Fatalf("an expired key is dropped at load")
	}
	if _, ok := got.KV["late"]; ok {
		t.Fatalf("a logged key that expired since is not replayed")
	}
	if string(got.KV["session"]) != "short" || got.Expirations["session"] != expiresAt {
		t.Fatalf("the logged expiration is kept: %q %d", got.KV["session"], got.Expirations["session"])
	}
	if !bytes.Equal(got.KV["big"], big) {
		t.Fatalf("entries of any size are replayed")
	}

	if strings.Join(got.Types, ",") != "books,authors" {
		t.Fatalf("types are repaired in the 0.2.4 order, got %v", got.Types)
	}

	ids := []string{}
	for _, d := range got.Entities["books"] {
		ids = append(ids, d.ID)
	}
	if strings.Join(ids, ",") != "b2,b3" {
		t.Fatalf("the id order is repaired like 0.2.4, got %v", ids)
	}

	without, err := legacy.Read(dir, false, now)
	if err != nil {
		t.Fatal(err)
	}
	if without.Replayed != 0 || len(without.Entities["books"]) != 2 {
		t.Fatalf("without crash recovery the logs are ignored, as in 0.2.4: %+v", without)
	}
}

func TestMigrationMovesTheLegacyFilesAndKeepsTheData(t *testing.T) {
	dir := t.TempDir()
	d := &legacy.Data{
		KV:          map[string][]byte{"k": []byte("v"), "ttl": []byte("t")},
		Expirations: map[string]int64{"ttl": time.Now().Unix() + 600},
		Types:       []string{"books"},
		Entities:    map[string][]legacy.Document{"books": {{ID: "b2", Doc: map[string]any{"id": "b2", "n": 2.0}}, {ID: "b1", Doc: map[string]any{"id": "b1"}}}},
	}
	if err := legacy.Write(dir, d); err != nil {
		t.Fatal(err)
	}

	globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: dir, Shards: 4}})
	storage.LoadDB()
	t.Cleanup(func() { _ = storage.Close() })

	report := storage.Engine().Migration()
	if report == nil || report.Documents != 2 || report.KVKeys != 2 || report.Entities != 1 {
		t.Fatalf("unexpected migration report %+v", report)
	}

	for _, name := range []string{legacy.DataFile, legacy.JsonDataFile, legacy.ExpirationFile} {
		if _, err := os.Stat(filepath.Join(dir, name)); !os.IsNotExist(err) {
			t.Fatalf("%s should be moved aside", name)
		}

		info, err := os.Stat(filepath.Join(dir, datastore.LegacyFolder, name))
		if err != nil || info.Mode().Perm() != 0o600 {
			t.Fatalf("%s should be kept in %s with mode 0600: %v", name, datastore.LegacyFolder, err)
		}
	}

	snap := storage.Engine().Snapshot("books")
	first := ""
	snap.Ascend(0, func(d datastore.Doc) bool {
		first = d.ID()
		return false
	})
	if snap.Len() != 2 || first != "b2" {
		t.Fatalf("documents keep their 0.2.4 order")
	}

	if v, err := storage.GetByKey("k"); err != nil || string(v) != "v" {
		t.Fatalf("kv keys are migrated: %q %v", v, err)
	}

	if exp, ok := storage.GetKeyExpiration("ttl"); !ok || exp != d.Expirations["ttl"] {
		t.Fatalf("expirations are migrated")
	}

	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}

	storage.LoadDB()
	if storage.Engine().Migration() != nil {
		t.Fatalf("a migrated folder is not migrated twice")
	}
	if storage.Engine().Snapshot("books").Len() != 2 {
		t.Fatalf("the data survives a restart")
	}
}

func TestExportLegacyWritesA024Folder(t *testing.T) {
	dir := t.TempDir()
	globals.SetConfig(&configuration.Config{Store: configuration.StoreConfig{Folder: dir, Shards: 4}})
	storage.LoadDB()
	t.Cleanup(func() { _ = storage.Close() })

	e := storage.Engine()
	if err := e.Update(func(tx *datastore.Tx) error {
		tx.EnsureType("books")
		_ = tx.Put("books", "b1", map[string]any{"id": "b1", "title": "Dune"})
		return tx.Put("books", "b0", map[string]any{"id": "b0"})
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.PutKeyValueWithTTL("k", []byte("v"), 600); err != nil {
		t.Fatal(err)
	}

	out := t.TempDir()
	if _, err := e.ExportLegacy(out); err != nil {
		t.Fatal(err)
	}

	back, err := legacy.Read(out, false, time.Now())
	if err != nil {
		t.Fatal(err)
	}

	if strings.Join(back.Types, ",") != "books" || len(back.Entities["books"]) != 2 || back.Entities["books"][0].ID != "b1" {
		t.Fatalf("unexpected export %+v", back)
	}

	if string(back.KV["k"]) != "v" || back.Expirations["k"] == 0 {
		t.Fatalf("kv keys and expirations are exported")
	}
}
