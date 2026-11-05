package compat_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/internal/durable"
)

const format2FixturesDir = "testdata/pr-format2"

func TestFoldersOfTheFormatTwoEngineAreRead(t *testing.T) {
	raw, err := os.ReadFile(legacyGoldenPath)
	if err != nil {
		t.Fatal(err)
	}

	var golden map[string][]goldenEntry
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatal(err)
	}
	want := golden["full"]

	for _, variant := range []string{"format2", "format2-crash", "e68e64f", "e68e64f-crash", "c104f4a", "c104f4a-crash"} {
		folder := copyFolder(t, filepath.Join(format2FixturesDir, variant))

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

			if strings.HasSuffix(variant, "-crash") {
				if got := s.run(fixtureRequest{Method: "GET", Path: "/kv/extra:key"}); got.Status != 200 || !strings.Contains(got.Body, `"value":"after the checkpoint"`) {
					t.Fatalf("%s round %d: the write after the last checkpoint is replayed, got %d %q", variant, round, got.Status, got.Body)
				}
			}

			shutdownEngine()

			manifest, err := os.ReadFile(filepath.Join(folder, durable.ManifestFile))
			if err != nil || !strings.Contains(string(manifest), `"format": 2`) {
				t.Fatalf("%s round %d: the manifest keeps format 2, got %s", variant, round, manifest)
			}

			if _, err := os.Stat(filepath.Join(folder, durable.PreviousManifest)); err != nil {
				t.Fatalf("%s round %d: the previous checkpoint is kept to fall back on: %v", variant, round, err)
			}
		}
	}
}
