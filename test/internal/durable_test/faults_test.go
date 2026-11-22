package durable_test

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/durable"
	"github.com/taymour/elysiandb/internal/journal"
)

type mixedLedger struct {
	mu       sync.Mutex
	docs     map[string]string
	gone     map[string]bool
	inFlight map[string]bool
}

func newMixedLedger() *mixedLedger {
	return &mixedLedger{docs: map[string]string{}, gone: map[string]bool{}, inFlight: map[string]bool{}}
}

func mixedOptions(dir string, fs *memFS, mode journal.Mode) datastore.Options {
	o := engineOptions(dir, fs)
	o.Durability = mode

	return o
}

func runMixedWorkload(e *datastore.Engine, l *mixedLedger, steps int) {
	for i := 0; i < steps; i++ {
		id := fmt.Sprintf("m%02d", i%17)
		value := fmt.Sprintf("v%d", i)
		l.inFlight[id] = true

		switch i % 7 {
		case 5:
			if err := e.Update(func(tx *datastore.Tx) error {
				tx.Delete("a", id)
				return nil
			}); err != nil {
				return
			}
			delete(l.docs, id)
			l.gone[id] = true
		case 6:
			other := fmt.Sprintf("o%02d", i%5)
			if err := e.Update(func(tx *datastore.Tx) error {
				tx.EnsureType("a")
				tx.EnsureType("b")
				if err := tx.Put("a", id, map[string]any{"id": id, "v": value}); err != nil {
					return err
				}
				return tx.Put("b", other, map[string]any{"id": other, "v": value})
			}); err != nil {
				return
			}
			l.docs[id] = value
			delete(l.gone, id)
		default:
			if err := e.Update(func(tx *datastore.Tx) error {
				tx.EnsureType("a")
				return tx.Put("a", id, map[string]any{"id": id, "v": value, "pad": strings.Repeat("p", 100)})
			}); err != nil {
				return
			}
			l.docs[id] = value
			delete(l.gone, id)
		}
		delete(l.inFlight, id)

		switch {
		case i%11 == 10:
			_ = e.Checkpoint()
		case i%19 == 18:
			_, _ = e.Compact(2)
		case i == 45:
			_, _ = e.CompactPages()
		case i%29 == 28:
			if err := e.Update(func(tx *datastore.Tx) error {
				tx.ResetEntity("b")
				return nil
			}); err != nil {
				return
			}
		case i%31 == 30:
			point := e.PointInTime()
			if snap := point.Snapshots["a"]; snap != nil {
				snap.ScanValues(func(_ datastore.Doc, _ []byte, err error) bool { return err == nil })
			}
			point.Release()
		}
	}
}

func checkMixed(t *testing.T, label string, e *datastore.Engine, l *mixedLedger) {
	t.Helper()

	snap := e.Snapshot("a")
	for id, want := range l.docs {
		d, ok := snap.Get(id)
		if !ok {
			if !l.inFlight[id] {
				t.Fatalf("%s: confirmed document %s is lost", label, id)
			}
			continue
		}

		doc, err := d.Decode()
		if err != nil {
			t.Fatalf("%s: document %s is unreadable: %v", label, id, err)
		}

		if got := doc["v"]; got != want && !l.inFlight[id] {
			t.Fatalf("%s: document %s is %v, confirmed %s", label, id, got, want)
		}
	}

	for id := range l.gone {
		if _, ok := snap.Get(id); ok && !l.inFlight[id] {
			t.Fatalf("%s: confirmed delete of %s is lost", label, id)
		}
	}

	snap.Ascend(0, func(d datastore.Doc) bool {
		if _, err := d.Decode(); err != nil {
			t.Fatalf("%s: stored document %s is unreadable: %v", label, d.ID(), err)
		}
		return true
	})
}

func crashMixed(t *testing.T, dir string, mode journal.Mode, crashAt, steps int) (*memFS, *mixedLedger) {
	t.Helper()

	fs := newMemFS()
	fs.crashAt = crashAt
	e, err := datastore.Open(mixedOptions(dir, fs, mode))
	if err != nil {
		if crashAt > 0 {
			return fs, newMixedLedger()
		}
		t.Fatal(err)
	}

	l := newMixedLedger()
	done := make(chan struct{})
	go func() {
		defer close(done)
		defer func() { _ = recover() }()
		runMixedWorkload(e, l, steps)
	}()

	select {
	case <-done:
		func() {
			defer func() { _ = recover() }()
			_ = e.Close()
		}()
		e.Abandon()
	case <-fs.down:
		e.Abandon()
		<-done
	}

	return fs, l
}

func reopenMixed(t *testing.T, label, dir string, image *memFS, mode journal.Mode, l *mixedLedger) {
	t.Helper()

	e, err := datastore.Open(mixedOptions(dir, image, mode))
	if err != nil {
		t.Fatalf("%s: cannot start: %v", label, err)
	}

	checkMixed(t, label, e, l)

	if err := e.Update(func(tx *datastore.Tx) error {
		tx.EnsureType("a")
		return tx.Put("a", "after", map[string]any{"id": "after", "v": "after"})
	}); err != nil {
		t.Fatalf("%s: cannot write after recovery: %v", label, err)
	}

	if err := e.Close(); err != nil {
		t.Fatalf("%s: cannot close after recovery: %v", label, err)
	}

	e, err = datastore.Open(mixedOptions(dir, image, mode))
	if err != nil {
		t.Fatalf("%s: cannot start twice: %v", label, err)
	}
	checkMixed(t, label+" (second start)", e, l)
	if _, ok := e.Snapshot("a").Get("after"); !ok {
		t.Fatalf("%s: the write after recovery is lost", label)
	}
	e.Abandon()
}

func TestProcessCrashAtEveryPointKeepsConfirmedWrites(t *testing.T) {
	const steps = 70
	dir := t.TempDir()

	clean, _ := crashMixed(t, dir, journal.ModeWrite, 0, steps)
	total := clean.Ops()
	if total < 150 {
		t.Fatalf("the workload should touch the disk many times, got %d", total)
	}

	for at := 1; at <= total; at += 1 + total/300 {
		fs, l := crashMixed(t, dir, journal.ModeWrite, at, steps)
		reopenMixed(t, fmt.Sprintf("crash at %d/%d", at, total), dir, fs.afterProcessCrash(), journal.ModeWrite, l)
	}
}

func TestPowerCutAtEveryPointKeepsSyncedWrites(t *testing.T) {
	const steps = 70
	dir := t.TempDir()

	clean, _ := crashMixed(t, dir, journal.ModeSync, 0, steps)
	total := clean.Ops()

	for at := 1; at <= total; at += 1 + total/300 {
		fs, l := crashMixed(t, dir, journal.ModeSync, at, steps)
		reopenMixed(t, fmt.Sprintf("power cut at %d/%d", at, total), dir, fs.afterPowerLoss(), journal.ModeSync, l)
	}
}

func TestConcurrentWritersShareTheirSyncs(t *testing.T) {
	fs := &gateFS{memFS: newMemFS()}
	dir := t.TempDir()
	o := datastore.Options{Folder: dir, Shards: 4, Durability: journal.ModeSync, FS: fs}
	e, err := datastore.Open(o)
	if err != nil {
		t.Fatal(err)
	}
	defer e.Abandon()

	const writers = 32
	const each = 20

	before := fs.syncs.Load()
	var wg sync.WaitGroup
	for w := 0; w < writers; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			for i := 0; i < each; i++ {
				id := fmt.Sprintf("w%02d-%03d", w, i)
				if err := e.Update(func(tx *datastore.Tx) error {
					tx.EnsureType("a")
					return tx.Put("a", id, map[string]any{"id": id})
				}); err != nil {
					t.Error(err)
					return
				}
			}
		}(w)
	}
	wg.Wait()

	syncs := fs.syncs.Load() - before
	if syncs >= writers*each {
		t.Fatalf("writers waiting together should share one sync: %d syncs for %d writes", syncs, writers*each)
	}

	if n := e.Snapshot("a").Len(); n != writers*each {
		t.Fatalf("%d documents", n)
	}
}

func logImage(t *testing.T, dir string) (*memFS, *mixedLedger) {
	t.Helper()

	fs := newMemFS()
	e, err := datastore.Open(mixedOptions(dir, fs, journal.ModeSync))
	if err != nil {
		t.Fatal(err)
	}

	l := newMixedLedger()
	runMixedWorkload(e, l, 30)
	_ = e.Checkpoint()
	for i := 0; i < 40; i++ {
		id := fmt.Sprintf("tail%02d", i)
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("a")
			return tx.Put("a", id, map[string]any{"id": id, "v": id})
		}); err != nil {
			t.Fatal(err)
		}
	}
	e.Abandon()

	return fs, l
}

func TestATornLogIsNeverReadAsValid(t *testing.T) {
	dir := t.TempDir()
	fs, l := logImage(t, dir)

	segments := fs.list(filepath.Join(dir, "log"))
	last := segments[len(segments)-1]

	var size int
	fs.mutate(last, func(n *memNode) { size = len(n.data) })

	for cut := size; cut > size-600 && cut > 0; cut -= 7 {
		for _, damage := range []string{"cut", "flip", "zero"} {
			image := fs.afterProcessCrash()
			image.mutate(last, func(n *memNode) {
				switch damage {
				case "cut":
					n.data = n.data[:cut]
				case "flip":
					if cut < len(n.data) {
						n.data[cut] ^= 0xa5
					}
				case "zero":
					for i := cut; i < len(n.data); i++ {
						n.data[i] = 0
					}
				}
				n.synced = append(n.synced[:0], n.data...)
			})

			e, err := datastore.Open(mixedOptions(dir, image, journal.ModeSync))
			if err != nil {
				t.Fatalf("%s at %d: cannot start: %v", damage, cut, err)
			}

			snap := e.Snapshot("a")
			seen := 0
			gap := false
			for i := 0; i < 40; i++ {
				d, ok := snap.Get(fmt.Sprintf("tail%02d", i))
				if !ok {
					gap = true
					continue
				}
				if gap {
					t.Fatalf("%s at %d: tail%02d follows a hole, the log was not replayed in order", damage, cut, i)
				}
				seen++
				if _, err := d.Decode(); err != nil {
					t.Fatalf("%s at %d: document unreadable: %v", damage, cut, err)
				}
			}

			for id, want := range l.docs {
				d, ok := snap.Get(id)
				if !ok {
					t.Fatalf("%s at %d: checkpointed document %s lost", damage, cut, id)
				}
				doc, _ := d.Decode()
				if doc["v"] != want {
					t.Fatalf("%s at %d: %s is %v", damage, cut, id, doc["v"])
				}
			}

			if err := e.Update(func(tx *datastore.Tx) error {
				tx.EnsureType("a")
				return tx.Put("a", "again", map[string]any{"id": "again", "v": "again"})
			}); err != nil {
				t.Fatalf("%s at %d: cannot write: %v", damage, cut, err)
			}
			e.Abandon()

			e, err = datastore.Open(mixedOptions(dir, image, journal.ModeSync))
			if err != nil {
				t.Fatalf("%s at %d: cannot start again: %v", damage, cut, err)
			}
			if _, ok := e.Snapshot("a").Get("again"); !ok {
				t.Fatalf("%s at %d: the write after the repair is lost", damage, cut)
			}
			if got := e.Snapshot("a").Len(); got < seen {
				t.Fatalf("%s at %d: documents went missing", damage, cut)
			}
			e.Abandon()
		}
	}
}

func TestDamageBeforeTheEndOfTheLogSetsTheRestAside(t *testing.T) {
	dir := t.TempDir()
	fs := newMemFS()
	o := mixedOptions(dir, fs, journal.ModeSync)
	o.SegmentSize = 2 << 10
	e, err := datastore.Open(o)
	if err != nil {
		t.Fatal(err)
	}

	for i := 0; i < 80; i++ {
		id := fmt.Sprintf("d%02d", i)
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("a")
			return tx.Put("a", id, map[string]any{"id": id, "pad": strings.Repeat("p", 120)})
		}); err != nil {
			t.Fatal(err)
		}
	}
	e.Abandon()

	segments := fs.list(filepath.Join(dir, "log"))
	if len(segments) < 3 {
		t.Fatalf("expected several segments, got %v", segments)
	}

	image := fs.afterPowerLoss()
	image.mutate(segments[1], func(n *memNode) { corrupt(n, len(n.data)/2) })

	e, err = datastore.Open(mixedOptions(dir, image, journal.ModeSync))
	if err != nil {
		t.Fatalf("a damaged log must not prevent a start: %v", err)
	}
	defer e.Abandon()

	if n := e.Snapshot("a").Len(); n == 0 || n >= 80 {
		t.Fatalf("the replay stops at the damage, %d documents", n)
	}

	damaged := 0
	for _, name := range image.list(filepath.Join(dir, "log")) {
		if strings.HasSuffix(name, ".damaged") {
			damaged++
		}
	}
	if damaged == 0 {
		t.Fatalf("what follows the damage is kept aside")
	}

	if err := e.Update(func(tx *datastore.Tx) error {
		return tx.Put("a", "next", map[string]any{"id": "next"})
	}); err != nil {
		t.Fatal(err)
	}
}

func TestAnInterruptedCheckpointAndAPartialPageAreIgnored(t *testing.T) {
	dir := t.TempDir()
	fs, l := logImage(t, dir)

	image := fs.afterProcessCrash()
	pages := image.list(filepath.Join(dir, "pages"))
	image.mutate(pages[len(pages)-1], func(n *memNode) {
		n.data = append(n.data, []byte("half a page that a checkpoint never finished")...)
		n.synced = append(n.synced[:0], n.data...)
	})

	for _, name := range []string{durable.ManifestFile + ".tmp", "kv-00000000000099999.ckpt.tmp"} {
		f, err := image.OpenFile(filepath.Join(dir, name), os.O_CREATE|os.O_WRONLY, 0o600)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = f.WriteAt([]byte("partial"), 0)
		_ = f.Close()
	}

	e, err := datastore.Open(mixedOptions(dir, image, journal.ModeSync))
	if err != nil {
		t.Fatal(err)
	}

	checkMixed(t, "interrupted checkpoint", e, l)
	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}

	e, err = datastore.Open(mixedOptions(dir, image, journal.ModeSync))
	if err != nil {
		t.Fatal(err)
	}
	checkMixed(t, "after the next checkpoint", e, l)
	e.Abandon()
}

func TestAFullDiskRefusesWritesAndComesBack(t *testing.T) {
	dir := t.TempDir()
	fs := newMemFS()
	e, err := datastore.Open(mixedOptions(dir, fs, journal.ModeSync))
	if err != nil {
		t.Fatal(err)
	}

	put := func(id string) error {
		return e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("a")
			return tx.Put("a", id, map[string]any{"id": id, "pad": strings.Repeat("x", 300)})
		})
	}

	if err := put("first"); err != nil {
		t.Fatal(err)
	}

	fs.SetQuota(fs.Usage())

	start := time.Now()
	err = put("no-room")
	if !errors.Is(err, datastore.ErrDiskFull) {
		t.Fatalf("a write on a full disk is refused with a clear error, got %v", err)
	}
	if took := time.Since(start); took > 10*time.Second {
		t.Fatalf("the refusal took %s", took)
	}

	start = time.Now()
	if err := put("next"); !errors.Is(err, datastore.ErrDiskFull) {
		t.Fatalf("the next write is refused too, got %v", err)
	}
	if took := time.Since(start); took > time.Second {
		t.Fatalf("the next refusal should be immediate, took %s", took)
	}

	if !errors.Is(e.KVPut("k", []byte("v"), 0), datastore.ErrDiskFull) {
		t.Fatalf("key writes are refused too")
	}

	if _, ok := e.Snapshot("a").Get("first"); !ok {
		t.Fatalf("reads are served on a full disk")
	}

	if e.RefusedWrites() < 3 {
		t.Fatalf("refusals are counted: %d", e.RefusedWrites())
	}

	fs.SetQuota(-1)
	deadline := time.Now().Add(10 * time.Second)
	for e.WriteRefusal() != nil {
		if time.Now().After(deadline) {
			t.Fatalf("writes never resume: %v", e.WriteRefusal())
		}
		time.Sleep(20 * time.Millisecond)
	}

	if err := put("after"); err != nil {
		t.Fatalf("writes resume by themselves: %v", err)
	}

	if err := e.Checkpoint(); err != nil {
		t.Fatalf("checkpoints resume: %v", err)
	}
	e.Abandon()

	image := fs.afterPowerLoss()
	e, err = datastore.Open(mixedOptions(dir, image, journal.ModeSync))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Abandon()

	snap := e.Snapshot("a")
	for _, id := range []string{"first", "after"} {
		if _, ok := snap.Get(id); !ok {
			t.Fatalf("%s is lost", id)
		}
	}
	if _, ok := snap.Get("next"); ok {
		t.Fatalf("a refused write must not appear")
	}
}

type failingFS struct {
	durable.FS
	left    atomic.Int64
	counted atomic.Int64
	failure error
}

func (f *failingFS) step() error {
	f.counted.Add(1)
	if f.left.Add(-1) < 0 {
		return f.failure
	}

	return nil
}

func (f *failingFS) OpenFile(name string, flag int, perm os.FileMode) (durable.File, error) {
	if err := f.step(); err != nil {
		return nil, err
	}

	file, err := f.FS.OpenFile(name, flag, perm)
	if err != nil {
		return nil, err
	}

	return &failingFile{File: file, fs: f}, nil
}

func (f *failingFS) Remove(name string) error {
	if err := f.step(); err != nil {
		return err
	}

	return f.FS.Remove(name)
}

func (f *failingFS) Rename(from, to string) error {
	if err := f.step(); err != nil {
		return err
	}

	return f.FS.Rename(from, to)
}

func (f *failingFS) MkdirAll(name string, perm os.FileMode) error {
	if err := f.step(); err != nil {
		return err
	}

	return f.FS.MkdirAll(name, perm)
}

func (f *failingFS) SyncDir(name string) error {
	if err := f.step(); err != nil {
		return err
	}

	return f.FS.SyncDir(name)
}

type failingFile struct {
	durable.File
	fs *failingFS
}

func (f *failingFile) WriteAt(p []byte, off int64) (int, error) {
	if err := f.fs.step(); err != nil {
		return 0, err
	}

	return f.File.WriteAt(p, off)
}

func (f *failingFile) Sync() error {
	if err := f.fs.step(); err != nil {
		return err
	}

	return f.File.Sync()
}

func copyTree(t *testing.T, from, to string) {
	t.Helper()

	entries, err := os.ReadDir(from)
	if err != nil {
		t.Fatal(err)
	}

	if err := os.MkdirAll(to, 0o755); err != nil {
		t.Fatal(err)
	}

	for _, entry := range entries {
		src, dst := filepath.Join(from, entry.Name()), filepath.Join(to, entry.Name())
		if entry.IsDir() {
			copyTree(t, src, dst)
			continue
		}

		in, err := os.Open(src)
		if err != nil {
			t.Fatal(err)
		}
		out, err := os.Create(dst)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = io.Copy(out, in)
		in.Close()
		out.Close()
	}
}

func snapshotState(t *testing.T, e *datastore.Engine) map[string]string {
	t.Helper()

	state := map[string]string{}
	for _, entity := range e.Types() {
		snap := e.Snapshot(entity)
		snap.Ascend(0, func(d datastore.Doc) bool {
			raw, err := d.Bytes()
			if err != nil {
				t.Fatal(err)
			}
			state[entity+"/"+d.ID()] = string(raw)
			return true
		})
	}

	for _, key := range e.KVKeys() {
		v, _, _ := e.KVGet(key)
		state["kv/"+key] = string(v)
	}

	return state
}

func TestAMigrationInterruptedAtAnyStepCompletesAtTheNextStart(t *testing.T) {
	fixture, err := filepath.Abs("../../compat/testdata/v0.2.4/full")
	if err != nil {
		t.Fatal(err)
	}

	reference := filepath.Join(t.TempDir(), "reference")
	copyTree(t, fixture, reference)
	e, err := datastore.Open(datastore.Options{Folder: reference, Shards: 8, Durability: journal.ModeWrite})
	if err != nil {
		t.Fatal(err)
	}
	want := snapshotState(t, e)
	if len(want) == 0 {
		t.Fatal("the fixture holds nothing")
	}
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}

	probe := filepath.Join(t.TempDir(), "probe")
	copyTree(t, fixture, probe)
	counter := &failingFS{FS: durable.OS, failure: syscall.EIO}
	counter.left.Store(1 << 40)
	e, err = datastore.Open(datastore.Options{Folder: probe, Shards: 8, Durability: journal.ModeWrite, FS: counter})
	if err != nil {
		t.Fatal(err)
	}
	total := int(counter.counted.Load())
	e.Abandon()
	if total < 20 {
		t.Fatalf("the migration should touch the disk many times, got %d", total)
	}

	for at := 0; at < total; at += 1 + total/80 {
		folder := filepath.Join(t.TempDir(), "folder")
		copyTree(t, fixture, folder)

		faulty := &failingFS{FS: durable.OS, failure: syscall.EIO}
		faulty.left.Store(int64(at))
		if e, err := datastore.Open(datastore.Options{Folder: folder, Shards: 8, Durability: journal.ModeWrite, FS: faulty}); err == nil {
			e.Abandon()
		}

		e, err := datastore.Open(datastore.Options{Folder: folder, Shards: 8, Durability: journal.ModeWrite})
		if err != nil {
			t.Fatalf("stopped at step %d of %d: the next start fails: %v", at, total, err)
		}

		got := snapshotState(t, e)
		e.Abandon()

		if len(got) != len(want) {
			t.Fatalf("stopped at step %d of %d: %d items, %d expected", at, total, len(got), len(want))
		}
		for k, v := range want {
			if got[k] != v {
				t.Fatalf("stopped at step %d of %d: %s is %q, %q expected", at, total, k, got[k], v)
			}
		}
	}
}

func TestPreallocatedSpaceChangesNothingInAFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "segment")

	f, err := durable.OS.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()

	durable.Preallocate(f, dir, 1<<20)
	if st, _ := f.Stat(); st.Size() != 0 {
		t.Fatalf("the visible size should not change, got %d", st.Size())
	}

	if _, err := f.WriteAt([]byte("hello"), 0); err != nil {
		t.Fatal(err)
	}

	durable.TrimPreallocated(f)

	st, err := f.Stat()
	if err != nil {
		t.Fatal(err)
	}
	if st.Size() != 5 {
		t.Fatalf("trimming should keep the data, size %d", st.Size())
	}

	got := make([]byte, 5)
	if _, err := f.ReadAt(got, 0); err != nil || string(got) != "hello" {
		t.Fatalf("%q %v", got, err)
	}
}
