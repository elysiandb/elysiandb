package compat_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/legacy/logengine"
)

const (
	format1FixturesDir = "testdata/pr-format1"
	format2FixturesDir = "testdata/pr-format2"
)

func checkPreviousFilesKept(t *testing.T, variant, folder string) {
	t.Helper()

	if logengine.Present(folder) {
		t.Fatalf("%s: the MANIFEST of the previous engine is moved aside", variant)
	}

	for _, name := range []string{logengine.ManifestFile, logengine.LogFolder} {
		if _, err := os.Stat(filepath.Join(folder, datastore.LegacyLogFolder, name)); err != nil {
			t.Fatalf("%s: %s is kept in %s: %v", variant, name, datastore.LegacyLogFolder, err)
		}
	}

	if _, err := os.Stat(filepath.Join(folder, datastore.PebbleFolder)); err != nil {
		t.Fatalf("%s: the store is written in %s: %v", variant, datastore.PebbleFolder, err)
	}

	for _, name := range []string{"users.key", "sessions.json"} {
		if _, err := os.Stat(filepath.Join(folder, name)); err != nil {
			t.Fatalf("%s: %s keeps its place: %v", variant, name, err)
		}
	}
}

func TestFoldersOfThePreviousEngineAreMigrated(t *testing.T) {
	raw, err := os.ReadFile(legacyGoldenPath)
	if err != nil {
		t.Fatal(err)
	}

	var golden map[string][]goldenEntry
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatal(err)
	}
	want := golden["full"]

	variants := map[string]string{
		"format1":       format1FixturesDir,
		"format1-crash": format1FixturesDir,
		"format2":       format2FixturesDir,
	}

	for variant, dir := range variants {
		folder := copyFolder(t, filepath.Join(dir, variant))

		for round := 0; round < 2; round++ {
			s := startServerOnFolder(t, folder, true)
			for i, r := range legacyReadRequests() {
				got := s.run(r)
				if intentionalChange(want[i].Request) {
					continue
				}
				if got.Status != want[i].Status || got.ContentType != want[i].ContentType || got.Body != want[i].Body {
					t.Errorf("%s round %d #%d %s %s\nwant %d %s\ngot  %d %s", variant, round, i, r.Method, r.Path, want[i].Status, truncate(want[i].Body), got.Status, truncate(got.Body))
				}
			}

			if variant == "format1-crash" {
				if got := s.run(fixtureRequest{Method: "GET", Path: "/kv/extra:key"}); got.Status != 200 || !strings.Contains(got.Body, `"value":"after the checkpoint"`) {
					t.Fatalf("%s round %d: the write after the last checkpoint is replayed, got %d %q", variant, round, got.Status, got.Body)
				}
			}

			shutdownEngine()
			checkPreviousFilesKept(t, variant, folder)
		}
	}
}

func TestCrashedFolderOfThePreviousEngineIsReplayed(t *testing.T) {
	folder := copyFolder(t, filepath.Join(format2FixturesDir, "format2-crash"))
	s := startServerOnFolder(t, folder, true)

	checks := []struct {
		req    fixtureRequest
		status int
		body   string
	}{
		{fixtureRequest{Method: "GET", Path: "/kv/extra:key"}, 200, `"value":"after the checkpoint"`},
		{fixtureRequest{Method: "GET", Path: "/api/note/n2"}, 200, `"text":"written after the checkpoint"`},
		{fixtureRequest{Method: "GET", Path: "/api/book/b02"}, 200, `"status":"crash-update"`},
		{fixtureRequest{Method: "GET", Path: "/api/book/b05"}, 404, ""},
		{fixtureRequest{Method: "GET", Path: "/api/book/count"}, 200, `{"count":29}`},
		{fixtureRequest{Method: "GET", Path: "/api/note/count"}, 200, `{"count":2}`},
	}

	for _, c := range checks {
		got := s.run(c.req)
		if got.Status != c.status || !strings.Contains(got.Body, c.body) {
			t.Errorf("%s %s: want %d containing %q, got %d %s", c.req.Method, c.req.Path, c.status, c.body, got.Status, truncate(got.Body))
		}
	}

	shutdownEngine()
	checkPreviousFilesKept(t, "format2-crash", folder)
}

type expectedDoc struct {
	ID   string `json:"id"`
	JSON string `json:"json"`
}

type expectedKV struct {
	Value     string `json:"value"`
	ExpiresAt int64  `json:"expires_at,omitempty"`
}

type expectedFolder struct {
	Types    []string                 `json:"types"`
	Entities map[string][]expectedDoc `json:"entities"`
	KV       map[string]expectedKV    `json:"kv"`
}

func TestRandomFolderOfThePreviousEngineIsMigratedDocumentByDocument(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join(format2FixturesDir, "random.expected.json"))
	if err != nil {
		t.Fatal(err)
	}

	var want expectedFolder
	if err := json.Unmarshal(raw, &want); err != nil {
		t.Fatal(err)
	}

	folder := copyFolder(t, filepath.Join(format2FixturesDir, "random"))
	for round := 0; round < 2; round++ {
		e, err := datastore.Open(datastore.Options{Folder: folder, Shards: 8, Durability: datastore.DurabilityWrite})
		if err != nil {
			t.Fatal(err)
		}

		if round == 0 {
			report := e.Migration()
			if report == nil || report.Replayed == 0 {
				t.Fatalf("the log written after the last checkpoint is replayed, report %+v", report)
			}
		}

		if got := e.Types(); strings.Join(got, ",") != strings.Join(want.Types, ",") {
			t.Fatalf("round %d: types %v, want %v", round, got, want.Types)
		}

		for entity, docs := range want.Entities {
			snap := e.Snapshot(entity)
			if snap.Len() != len(docs) {
				t.Fatalf("round %d: %s has %d documents, want %d", round, entity, snap.Len(), len(docs))
			}

			i := 0
			snap.Ascend(0, func(d datastore.Doc) bool {
				b, _ := d.Bytes()
				if d.ID() != docs[i].ID || string(b) != docs[i].JSON {
					t.Fatalf("round %d: %s #%d is %s %s, want %s %s", round, entity, i, d.ID(), b, docs[i].ID, docs[i].JSON)
				}
				i++
				return true
			})
		}

		keys := e.KVKeys()
		if len(keys) != len(want.KV) {
			t.Fatalf("round %d: %d keys, want %d", round, len(keys), len(want.KV))
		}

		for key, kv := range want.KV {
			value, exp, ok := e.KVGet(key)
			if !ok || strings.ToValidUTF8(string(value), "\uFFFD") != kv.Value || exp != kv.ExpiresAt {
				t.Fatalf("round %d: key %s is %q (%d), want %q (%d)", round, key, value, exp, kv.Value, kv.ExpiresAt)
			}
		}

		if err := e.Close(); err != nil {
			t.Fatal(err)
		}
	}
}
