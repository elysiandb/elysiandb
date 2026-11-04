package compat_test

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/legacy/logengine"
)

func TestReaderOfThePreviousEngine(t *testing.T) {
	for _, c := range []struct {
		dir      string
		replayed bool
	}{
		{filepath.Join(format1FixturesDir, "format1"), false},
		{filepath.Join(format1FixturesDir, "format1-crash"), true},
		{filepath.Join(format2FixturesDir, "format2"), false},
		{filepath.Join(format2FixturesDir, "format2-crash"), true},
		{filepath.Join(format2FixturesDir, "random"), true},
	} {
		if !logengine.Present(c.dir) {
			t.Fatalf("%s holds a MANIFEST", c.dir)
		}

		files := logengine.FilesIn(c.dir)
		if !slices.Contains(files, logengine.ManifestFile) || !slices.Contains(files, logengine.LogFolder) || slices.Contains(files, "users.key") {
			t.Fatalf("%s: unexpected files to move aside %v", c.dir, files)
		}

		f, err := logengine.Open(c.dir, time.Now())
		if err != nil {
			t.Fatalf("%s: %v", c.dir, err)
		}

		if (f.Replayed() > 0) != c.replayed {
			t.Fatalf("%s: %d records replayed", c.dir, f.Replayed())
		}

		docs := 0
		for _, entity := range f.Entities() {
			ids := map[string]bool{}
			if err := f.Each(entity, func(doc logengine.Document) error {
				if ids[doc.ID] || !strings.HasPrefix(string(doc.Value), "{") {
					t.Fatalf("%s: %s has a duplicate or invalid document %s", c.dir, entity, doc.ID)
				}
				ids[doc.ID] = true
				docs++
				return nil
			}); err != nil {
				t.Fatalf("%s: %v", c.dir, err)
			}
			_ = f.Refs(entity)
		}

		keys := 0
		if err := f.KV(func(kv logengine.KV) error {
			keys++
			return nil
		}); err != nil {
			t.Fatal(err)
		}

		if docs == 0 || keys == 0 || len(f.Types()) == 0 {
			t.Fatalf("%s: %d documents, %d keys, types %v", c.dir, docs, keys, f.Types())
		}
		f.Close()
	}
}

func damage(t *testing.T, path string, fn func(b []byte) []byte) {
	t.Helper()

	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	if err := os.WriteFile(path, fn(b), 0o600); err != nil {
		t.Fatal(err)
	}
}

func flipAt(offset int) func(b []byte) []byte {
	return func(b []byte) []byte {
		if offset < 0 {
			offset += len(b)
		}
		b[offset] ^= 0xff
		return b
	}
}

func TestDamagedFoldersOfThePreviousEngineAreLeftUntouched(t *testing.T) {
	random := filepath.Join(format2FixturesDir, "random")
	cases := map[string]func(dir string){
		"manifest not json": func(dir string) {
			damage(t, filepath.Join(dir, logengine.ManifestFile), func([]byte) []byte { return []byte("{") })
		},
		"unknown manifest format": func(dir string) {
			damage(t, filepath.Join(dir, logengine.ManifestFile), func(b []byte) []byte {
				return []byte(strings.Replace(string(b), `"format": 2`, `"format": 9`, 1))
			})
		},
		"kv checkpoint checksum": func(dir string) {
			matches, _ := filepath.Glob(filepath.Join(dir, "kv-*.ckpt"))
			damage(t, matches[0], flipAt(20))
		},
		"batch of a sealed segment": func(dir string) {
			damage(t, filepath.Join(dir, logengine.LogFolder, "000011.seg"), flipAt(-20))
		},
		"segment header": func(dir string) {
			damage(t, filepath.Join(dir, logengine.LogFolder, "000012.seg"), flipAt(0))
		},
		"page checksum": func(dir string) {
			raw, err := os.ReadFile(filepath.Join(dir, logengine.ManifestFile))
			if err != nil {
				t.Fatal(err)
			}
			var m struct {
				Entities []struct {
					Slots struct {
						ID uint64 `json:"id"`
					} `json:"slots"`
				} `json:"entities"`
			}
			if err := json.Unmarshal(raw, &m); err != nil || len(m.Entities) == 0 {
				t.Fatalf("cannot read the manifest: %v", err)
			}
			root := m.Entities[0].Slots.ID
			damage(t, filepath.Join(dir, logengine.PagesFolder, fmt.Sprintf("%06d.pgs", root>>32)), flipAt(int(uint32(root))+10))
		},
		"missing values": func(dir string) {
			damage(t, filepath.Join(dir, logengine.LogFolder, "000009.seg"), func(b []byte) []byte { return b[:64] })
		},
	}

	for name, apply := range cases {
		dir := copyFolder(t, random)
		apply(dir)

		e, err := datastore.Open(datastore.Options{Folder: dir, Shards: 8, Durability: datastore.DurabilityWrite})
		if err == nil {
			_ = e.Close()
			t.Fatalf("%s: the migration of a damaged folder fails", name)
		}

		if !logengine.Present(dir) {
			t.Fatalf("%s: the MANIFEST stays in place", name)
		}
		for _, left := range []string{datastore.PebbleFolder, ".migration", datastore.LegacyLogFolder} {
			if _, err := os.Stat(filepath.Join(dir, left)); err == nil {
				t.Fatalf("%s: a failed migration leaves no %s", name, left)
			}
		}
	}
}

func TestTornTailOfThePreviousEngineIsDropped(t *testing.T) {
	whole, err := logengine.Open(filepath.Join(format2FixturesDir, "random"), time.Now())
	if err != nil {
		t.Fatal(err)
	}
	replayed := whole.Replayed()
	whole.Close()

	dir := copyFolder(t, filepath.Join(format2FixturesDir, "random"))
	damage(t, filepath.Join(dir, logengine.LogFolder, "000012.seg"), func(b []byte) []byte { return b[:len(b)-5] })

	torn, err := logengine.Open(dir, time.Now())
	if err != nil {
		t.Fatalf("a torn last batch is dropped, got %v", err)
	}
	if torn.Replayed() >= replayed {
		t.Fatalf("the torn batch is not replayed: %d of %d records", torn.Replayed(), replayed)
	}
	torn.Close()

	e, err := datastore.Open(datastore.Options{Folder: dir, Shards: 8, Durability: datastore.DurabilityWrite})
	if err != nil {
		t.Fatalf("a folder with a torn tail migrates, got %v", err)
	}
	defer e.Close()

	if report := e.Migration(); report == nil || report.Replayed != torn.Replayed() || report.From != "log-pages" {
		t.Fatalf("unexpected migration report %+v", report)
	}
}
