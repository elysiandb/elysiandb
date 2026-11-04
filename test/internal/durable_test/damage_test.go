package durable_test

import (
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/durable"
)

func filledDisk(t *testing.T, dir string) (*memFS, *ledger) {
	t.Helper()

	fs := newMemFS()
	e, err := datastore.Open(engineOptions(dir, fs))
	if err != nil {
		t.Fatal(err)
	}

	l := newLedger()
	runWorkload(e, l, 60)
	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}

	for i := 0; i < 8; i++ {
		id := fmt.Sprintf("late%d", i)
		if err := e.Update(func(tx *datastore.Tx) error {
			return tx.Put("a", id, map[string]any{"v": id})
		}); err != nil {
			t.Fatal(err)
		}
		l.docs[id] = id
	}
	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}
	e.Abandon()

	return fs, l
}

func corrupt(n *memNode, at int) {
	if at < 0 {
		at += len(n.data)
	}
	for i := at; i < at+16 && i < len(n.data); i++ {
		n.data[i] ^= 0x5a
	}
	n.synced = append(n.synced[:0], n.data...)
}

func TestDamagedFilesNeverPreventAStart(t *testing.T) {
	cases := []struct {
		name   string
		damage func(fs *memFS, dir string)
		keeps  bool
	}{
		{"unreadable manifest", func(fs *memFS, dir string) {
			fs.mutate(filepath.Join(dir, durable.ManifestFile), func(n *memNode) { corrupt(n, 2) })
		}, true},
		{"truncated manifest", func(fs *memFS, dir string) {
			fs.mutate(filepath.Join(dir, durable.ManifestFile), func(n *memNode) { n.data = n.data[:len(n.data)/2]; n.synced = n.data })
		}, true},
		{"missing key value checkpoint", func(fs *memFS, dir string) {
			kv := fs.list(filepath.Join(dir, "kv-"))
			_ = fs.Remove(kv[len(kv)-1])
		}, true},
		{"corrupt key value checkpoint", func(fs *memFS, dir string) {
			kv := fs.list(filepath.Join(dir, "kv-"))
			fs.mutate(kv[len(kv)-1], func(n *memNode) { corrupt(n, len(n.data)/2) })
		}, true},
		{"corrupt latest page segment", func(fs *memFS, dir string) {
			pages := fs.list(filepath.Join(dir, durable.PagesFolder))
			fs.mutate(pages[len(pages)-1], func(n *memNode) { corrupt(n, -40) })
		}, true},
		{"torn end of the log", func(fs *memFS, dir string) {
			logs := fs.list(filepath.Join(dir, "log"))
			fs.mutate(logs[len(logs)-1], func(n *memNode) { n.data = n.data[:len(n.data)-5]; n.synced = n.data })
		}, false},
		{"garbage after the log", func(fs *memFS, dir string) {
			logs := fs.list(filepath.Join(dir, "log"))
			fs.mutate(logs[len(logs)-1], func(n *memNode) {
				n.data = append(n.data, []byte(strings.Repeat("\xde\xad", 40))...)
				n.synced = n.data
			})
		}, true},
		{"corrupt record inside the log", func(fs *memFS, dir string) {
			logs := fs.list(filepath.Join(dir, "log"))
			fs.mutate(logs[len(logs)-1], func(n *memNode) { corrupt(n, len(n.data)/2) })
		}, false},
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			dir := t.TempDir()
			fs, l := filledDisk(t, dir)
			c.damage(fs, dir)

			e, err := datastore.Open(engineOptions(dir, fs))
			if err != nil {
				t.Fatalf("the engine must start: %v", err)
			}

			if c.keeps {
				checkLedger(t, c.name, e, l)
			}

			if err := e.Update(func(tx *datastore.Tx) error {
				return tx.Put("a", "fresh", map[string]any{"v": "fresh"})
			}); err != nil {
				t.Fatalf("the engine must accept writes: %v", err)
			}
			if err := e.Checkpoint(); err != nil {
				t.Fatalf("the engine must checkpoint again: %v", err)
			}
			if err := e.Close(); err != nil {
				t.Fatal(err)
			}

			e, err = datastore.Open(engineOptions(dir, fs))
			if err != nil {
				t.Fatalf("the engine must start again after repairing: %v", err)
			}
			if _, ok := e.Snapshot("a").Get("fresh"); !ok {
				t.Fatalf("a write after the repair must survive")
			}
			e.Abandon()
		})
	}
}

func TestAFullDiskNeverAcknowledgesALostWrite(t *testing.T) {
	dir := t.TempDir()
	fs := newMemFS()
	e, err := datastore.Open(engineOptions(dir, fs))
	if err != nil {
		t.Fatal(err)
	}

	l := newLedger()
	runWorkload(e, l, 20)

	fs.SetQuota(fs.Usage())
	done := make(chan error, 1)
	go func() {
		done <- e.Update(func(tx *datastore.Tx) error {
			return tx.Put("a", "blocked", map[string]any{"v": "blocked", "pad": strings.Repeat("x", 300)})
		})
	}()

	select {
	case err := <-done:
		t.Fatalf("a write must not be acknowledged while the disk is full, got %v", err)
	case <-time.After(300 * time.Millisecond):
	}

	if err := e.Checkpoint(); err == nil {
		t.Fatalf("a checkpoint cannot succeed on a full disk")
	}

	if _, ok := e.Snapshot("a").Get("d00"); !ok {
		t.Fatalf("reads keep working while the disk is full")
	}

	fs.SetQuota(-1)
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("the write must go through once the disk has room: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatalf("the write never went through after the disk was freed")
	}
	l.docs["blocked"] = "blocked"

	if err := e.Checkpoint(); err != nil {
		t.Fatalf("checkpoints resume once the disk has room: %v", err)
	}
	e.Abandon()

	image := fs.afterPowerLoss()
	e, err = datastore.Open(engineOptions(dir, image))
	if err != nil {
		t.Fatal(err)
	}
	checkLedger(t, "after a full disk", e, l)
	e.Abandon()
}
