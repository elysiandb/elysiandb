package datastore_test

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/journal"
)

func TestStatsAnswerDuringACheckpoint(t *testing.T) {
	opts := options(t.TempDir())
	opts.SegmentSize = 0
	e, err := datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	putDocs(t, e, "big", 0, 60000, workDoc)
	for _, f := range []string{"a", "b", "c", "text"} {
		e.EnsureIndex("big", datastore.IndexSpec{Kind: datastore.IndexFilter, Field: f})
	}

	var slowest, longestCheckpoint time.Duration
	for round := 0; round < 3; round++ {
		putDocs(t, e, "big", 60000+round, 60001+round, workDoc)

		done := make(chan time.Duration, 1)
		go func() {
			t0 := time.Now()
			if err := e.Checkpoint(); err != nil {
				t.Error(err)
			}
			done <- time.Since(t0)
		}()

		for waiting := true; waiting; {
			select {
			case d := <-done:
				longestCheckpoint = max(longestCheckpoint, d)
				waiting = false
			default:
				t0 := time.Now()
				_ = e.Stats()
				slowest = max(slowest, time.Since(t0))
				time.Sleep(time.Millisecond)
			}
		}
	}

	if longestCheckpoint < 20*time.Millisecond {
		t.Skipf("checkpoints too fast to observe (%s)", longestCheckpoint)
	}
	if slowest > longestCheckpoint/4 {
		t.Fatalf("Stats waited %s during a checkpoint of %s", slowest, longestCheckpoint)
	}
}

func dirBytes(t *testing.T, dir string) int64 {
	t.Helper()

	var total int64
	entries, err := os.ReadDir(dir)
	if err != nil {
		return 0
	}
	for _, entry := range entries {
		info, err := entry.Info()
		if err == nil {
			total += info.Size()
		}
	}

	return total
}

func TestDiskIsGivenBackAfterAnEntityIsEmptied(t *testing.T) {
	dir := t.TempDir()
	e, err := datastore.Open(datastore.Options{Folder: dir, Shards: 4, Durability: journal.ModeBackground, PageSegmentSize: 1 << 20, SegmentSize: 1 << 20, Background: true})
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	e.EnsureIndex("a", datastore.IndexSpec{Kind: datastore.IndexFilter, Field: "v"})
	for i := 0; i < 40000; i += 500 {
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("a")
			for j := i; j < i+500; j++ {
				if err := tx.Put("a", fmt.Sprintf("doc-%06d", j), map[string]any{"v": fmt.Sprintf("value-%d", j), "pad": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}

	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}

	pages := filepath.Join(dir, "pages")
	full := dirBytes(t, pages)
	if full < 1<<20 {
		t.Fatalf("the pages should hold several segments, got %d bytes", full)
	}

	if err := e.Update(func(tx *datastore.Tx) error {
		tx.ResetEntity("a")
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	deadline := time.Now().Add(30 * time.Second)
	for dirBytes(t, pages) > full/4 {
		if time.Now().After(deadline) {
			t.Fatalf("the pages of an emptied entity should be removed without further writes: %d bytes, %d before", dirBytes(t, pages), full)
		}
		time.Sleep(50 * time.Millisecond)
	}
}

func TestAResetKeepsEveryKnownIndexFollowingTheFill(t *testing.T) {
	e, err := datastore.Open(datastore.Options{Folder: t.TempDir(), Shards: 4, Durability: journal.ModeBackground})
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	put := func(id, v string) {
		t.Helper()
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("a")
			return tx.Put("a", id, map[string]any{"v": v})
		}); err != nil {
			t.Fatal(err)
		}
	}

	spec := datastore.IndexSpec{Kind: datastore.IndexFilter, Field: "v"}
	put("x", "one")
	e.EnsureIndex("a", spec)
	e.DropIdleIndexes(-time.Hour)
	e.DropIdleIndexes(-time.Hour)

	if !e.Snapshot("a").IndexDormant(spec) {
		t.Fatal("the index should be released")
	}

	if err := e.Update(func(tx *datastore.Tx) error {
		tx.ResetEntity("a")
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	put("y", "two")
	view, ok := e.Snapshot("a").Index(spec)
	if !ok || view.Len() == 0 {
		t.Fatalf("the index should have followed the insert after the reset: %v", ok)
	}
}
