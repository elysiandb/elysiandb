package durable_test

import (
	"io"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/taymour/elysiandb/internal/durable"
)

func TestKVFileRoundTripAndCorruption(t *testing.T) {
	fs := newMemFS()
	_ = fs.MkdirAll("/d", 0o755)
	path := filepath.Join("/d", "kv-1.ckpt")
	records := []durable.KVRecord{
		{Key: "a", LSN: 1, ExpiresAt: 0, Segment: 1, Offset: 16, Length: 5},
		{Key: "é:long", LSN: 1 << 40, ExpiresAt: -3, Segment: 9, Offset: 1 << 20, Length: 0},
	}

	if err := durable.WriteKVFile(fs, path, records); err != nil {
		t.Fatal(err)
	}

	got, err := durable.ReadKVFile(fs, path)
	if err != nil || !reflect.DeepEqual(got, records) {
		t.Fatalf("round trip: %+v, %v", got, err)
	}

	fs.mutate(path, func(n *memNode) { n.data[12] ^= 0xff })
	if _, err := durable.ReadKVFile(fs, path); err == nil {
		t.Fatalf("a corrupt file must be detected")
	}
}

func TestAtomicWritesLeaveTheOldFileUntilTheyAreComplete(t *testing.T) {
	fs := newMemFS()
	_ = fs.MkdirAll("/d", 0o755)
	path := filepath.Join("/d", "file")

	write := func(s string) error {
		return durable.WriteFileAtomic(fs, path, func(w io.Writer) error {
			_, err := w.Write([]byte(s))
			return err
		})
	}

	if err := write("first"); err != nil {
		t.Fatal(err)
	}

	base := fs.Ops()
	for at := 1; at < 6; at++ {
		crashed := fs.afterProcessCrash()
		crashed.ops = 0
		crashed.crashAt = at
		_ = durable.WriteFileAtomic(crashed, path, func(w io.Writer) error {
			_, err := w.Write([]byte("second"))
			return err
		})

		image := crashed.afterPowerLoss()
		data, err := durable.ReadFile(image, path)
		if err != nil || (string(data) != "first" && string(data) != "second") {
			t.Fatalf("crash at operation %d (after %d): the file must be whole, got %q, %v", at, base, data, err)
		}
	}
}

func TestManifestGenerations(t *testing.T) {
	fs := newMemFS()
	_ = fs.MkdirAll("/d", 0o755)
	m := durable.OpenManifests(fs, "/d")

	stored, err := m.Stored()
	if err != nil || len(stored) != 0 {
		t.Fatalf("a new folder has no manifest: %v %v", stored, err)
	}

	for _, content := range []string{"one", "two", "three"} {
		if err := m.Commit([]byte(content)); err != nil {
			t.Fatal(err)
		}
	}

	stored, err = m.Stored()
	if err != nil || len(stored) != 2 || string(stored[0].Data) != "three" || string(stored[1].Data) != "two" {
		t.Fatalf("the last two generations are kept, newest first: %+v, %v", stored, err)
	}

	_ = fs.Remove(filepath.Join("/d", durable.ManifestFile))
	stored, err = durable.OpenManifests(fs, "/d").Stored()
	if err != nil || len(stored) != 1 || string(stored[0].Data) != "two" {
		t.Fatalf("the previous generation stays readable alone: %+v, %v", stored, err)
	}
}
