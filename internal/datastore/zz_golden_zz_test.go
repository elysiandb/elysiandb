package datastore

import (
	"path/filepath"
	"testing"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func TestGenerateGoldenZManifest(t *testing.T) {
	out := filepath.Join(goldenRoot(t), "persistence")
	if err := goldenio.WriteManifest(out, "persistence"); err != nil {
		t.Fatal(err)
	}
}
