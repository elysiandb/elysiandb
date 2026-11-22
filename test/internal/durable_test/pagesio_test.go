package durable_test

import (
	"fmt"
	"os"
	"sync/atomic"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/durable"
	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/pagetree"
)

type gateFS struct {
	*memFS
	gate  chan struct{}
	syncs atomic.Int64
}

type gateFile struct {
	journal.File
	fs *gateFS
}

func (g *gateFS) OpenFile(name string, flag int, perm os.FileMode) (journal.File, error) {
	f, err := g.memFS.OpenFile(name, flag, perm)
	if err != nil {
		return nil, err
	}

	return &gateFile{File: f, fs: g}, nil
}

func (f *gateFile) Sync() error {
	f.fs.syncs.Add(1)
	if gate := f.fs.gate; gate != nil {
		<-gate
	}

	return f.File.Sync()
}

func fillTree(t *testing.T, store *durable.PageStore, n int, padding int) pagetree.Tree {
	t.Helper()

	tok := pagetree.NewToken()
	tree := pagetree.New(store).WithToken(tok)
	value := make([]byte, padding)
	for i := 0; i < n; i++ {
		tree, _ = tree.Put(fmt.Sprintf("key-%08d", i), value)
	}
	tok.Freeze()

	return tree.WithToken(nil)
}

func TestPageReadsDoNotWaitForASync(t *testing.T) {
	fs := &gateFS{memFS: newMemFS()}
	dir := t.TempDir()
	store, err := durable.OpenPageStore(fs, dir, 1<<30, 0)
	if err != nil {
		t.Fatal(err)
	}

	tree := fillTree(t, store, 2000, 32)
	if err := store.FlushTree(tree); err != nil {
		t.Fatal(err)
	}

	fs.gate = make(chan struct{})
	synced := make(chan error, 1)
	go func() { synced <- store.Sync() }()

	for fs.syncs.Load() == 0 {
		time.Sleep(time.Millisecond)
	}

	store.SetLimit(1)
	store.SetLimit(1 << 30)

	done := make(chan struct{})
	go func() {
		defer close(done)
		for i := 0; i < 2000; i += 97 {
			if _, ok := tree.Get(fmt.Sprintf("key-%08d", i)); !ok {
				t.Errorf("key %d is missing", i)
			}
		}
	}()

	select {
	case <-done:
	case <-time.After(10 * time.Second):
		t.Fatal("page reads wait for the sync of the checkpoint")
	}

	close(fs.gate)
	if err := <-synced; err != nil {
		t.Fatal(err)
	}
}

func TestPagesAreSyncedInSlicesWithAPause(t *testing.T) {
	fs := &gateFS{memFS: newMemFS()}
	store, err := durable.OpenPageStore(fs, t.TempDir(), 1<<30, 0)
	if err != nil {
		t.Fatal(err)
	}

	var pauses atomic.Int64
	store.Pace(func(time.Duration) time.Duration {
		pauses.Add(1)
		return time.Millisecond
	})

	tree := fillTree(t, store, 60000, 256)
	if err := store.FlushTree(tree); err != nil {
		t.Fatal(err)
	}

	if pauses.Load() < 2 {
		t.Fatalf("a large flush should settle more than once, got %d pauses", pauses.Load())
	}

	if fs.syncs.Load() != pauses.Load() {
		t.Fatalf("each slice is synced before its pause: %d syncs, %d pauses", fs.syncs.Load(), pauses.Load())
	}

	if err := store.Sync(); err != nil {
		t.Fatal(err)
	}
	store.SetLimit(1)
	if v, ok := tree.Get(fmt.Sprintf("key-%08d", 59999)); !ok || len(v) != 256 {
		t.Fatalf("the last key reads back wrong: %d %v", len(v), ok)
	}
}
