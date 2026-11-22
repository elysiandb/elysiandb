package compat_test

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/backup"
)

const archivesDir = "testdata/archives"

func goldenFull(t *testing.T) []goldenEntry {
	t.Helper()

	raw, err := os.ReadFile(legacyGoldenPath)
	if err != nil {
		t.Fatal(err)
	}

	var golden map[string][]goldenEntry
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatal(err)
	}

	return golden["full"]
}

func compareWithGolden(t *testing.T, label string, s *corpusServer, want []goldenEntry) {
	t.Helper()

	for i, r := range legacyReadRequests() {
		if intentionalChange(want[i].Request) {
			continue
		}

		got := s.run(r)
		if got.Status != want[i].Status || got.ContentType != want[i].ContentType || got.Body != want[i].Body {
			t.Errorf("%s #%d %s %s\nwant %d %s\ngot  %d %s", label, i, r.Method, r.Path, want[i].Status, truncate(want[i].Body), got.Status, truncate(got.Body))
		}
	}
}

func TestArchivesOfEarlierVersionsAreRestoredIdentically(t *testing.T) {
	want := goldenFull(t)

	for _, name := range []string{"v0.2.4.tar.gz", "c104f4a.tar.gz"} {
		folder := t.TempDir()
		startServerOnFolder(t, folder, true)

		f, err := os.Open(filepath.Join(archivesDir, name))
		if err != nil {
			t.Fatal(err)
		}

		if _, err := backup.Restore(f); err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		f.Close()
		shutdownEngine()

		for round := 0; round < 2; round++ {
			s := startServerOnFolder(t, folder, true)
			compareWithGolden(t, fmt.Sprintf("%s round %d", name, round), s, want)
			shutdownEngine()
		}
	}
}

func readArchive(t *testing.T, data []byte) ([]string, map[string][]byte) {
	t.Helper()

	gz, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}

	names := []string{}
	files := map[string][]byte{}
	tr := tar.NewReader(gz)
	for {
		h, err := tr.Next()
		if err == io.EOF {
			return names, files
		}
		if err != nil {
			t.Fatal(err)
		}

		content, err := io.ReadAll(tr)
		if err != nil {
			t.Fatal(err)
		}

		names = append(names, h.Name)
		files[h.Name] = content
	}
}

func TestAnArchiveHoldsTheSameFilesAsThoseOfEarlierVersions(t *testing.T) {
	fixture, err := os.ReadFile(filepath.Join(archivesDir, "v0.2.4.tar.gz"))
	if err != nil {
		t.Fatal(err)
	}
	wantNames, wantFiles := readArchive(t, fixture)

	startServerOnFolder(t, copyFolder(t, filepath.Join(legacyFixturesDir, "full")), true)

	var out bytes.Buffer
	if _, err := backup.Create(&out, time.Now()); err != nil {
		t.Fatal(err)
	}
	gotNames, gotFiles := readArchive(t, out.Bytes())

	if !reflect.DeepEqual(gotNames, wantNames) {
		t.Fatalf("the files or their order changed:\n%v\n%v", gotNames, wantNames)
	}

	for _, name := range wantNames {
		if name == "manifest.json" {
			continue
		}
		if !bytes.Equal(gotFiles[name], wantFiles[name]) {
			t.Errorf("%s differs:\n%s\n%s", name, truncate(string(gotFiles[name])), truncate(string(wantFiles[name])))
		}
	}

	var gotManifest, wantManifest map[string]any
	if err := json.Unmarshal(gotFiles["manifest.json"], &gotManifest); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(wantFiles["manifest.json"], &wantManifest); err != nil {
		t.Fatal(err)
	}
	for _, varying := range []string{"created_at", "elysiandb_version"} {
		delete(gotManifest, varying)
		delete(wantManifest, varying)
	}
	if !reflect.DeepEqual(gotManifest, wantManifest) {
		t.Fatalf("the manifest differs:\n%v\n%v", gotManifest, wantManifest)
	}
}

func TestAnArchiveOfThisVersionIsRestoredByVersion024(t *testing.T) {
	binary := os.Getenv("ELYSIANDB_V024_BINARY")
	if binary == "" {
		t.Skip("needs a binary of 0.2.4 in ELYSIANDB_V024_BINARY")
	}

	want := goldenFull(t)
	startServerOnFolder(t, copyFolder(t, filepath.Join(legacyFixturesDir, "full")), true)

	archive := filepath.Join(t.TempDir(), "archive.tar.gz")
	f, err := os.Create(archive)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := backup.Create(f, time.Now()); err != nil {
		t.Fatal(err)
	}
	f.Close()
	shutdownEngine()

	folder := t.TempDir()
	config := filepath.Join(t.TempDir(), "config.yaml")
	yaml := fmt.Sprintf("store:\n  folder: %s\n  shards: 8\n  flushIntervalSeconds: 0\n  crashRecovery: { enabled: true, maxLogMB: 100 }\nengine:\n  name: internal\nserver:\n  http: { enabled: false, host: 127.0.0.1, port: 1 }\n  tcp: { enabled: false, host: 127.0.0.1, port: 2 }\nsecurity:\n  authentication: { enabled: true, mode: user }\napi:\n  schema: { enabled: true, strict: true }\n  hooks: { enabled: true }\nlog: { level: error, format: text }\n", folder)
	if err := os.WriteFile(config, []byte(yaml), 0o600); err != nil {
		t.Fatal(err)
	}

	if out, err := exec.Command(binary, "--config", config, "restore", archive, "--force").CombinedOutput(); err != nil {
		t.Fatalf("0.2.4 refuses the archive: %v\n%s", err, out)
	}

	compareWithGolden(t, "folder written by 0.2.4 from an archive of this version", startServerOnFolder(t, folder, true), want)
}
