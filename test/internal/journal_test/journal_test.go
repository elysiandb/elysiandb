package journal_test

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"syscall"
	"testing"

	"github.com/taymour/elysiandb/internal/journal"
)

func record(i int) journal.Record {
	return journal.Record{Kind: journal.KindDocPut, Entity: "books", Key: fmt.Sprintf("id-%05d", i), Gen: 1, Value: []byte(fmt.Sprintf(`{"n":%d}`, i))}
}

func openJournal(t *testing.T, dir string, opts journal.Options) *journal.Journal {
	t.Helper()

	j, err := journal.Open(dir, opts)
	if err != nil {
		t.Fatal(err)
	}

	return j
}

func replayAll(t *testing.T, j *journal.Journal) ([]journal.Record, journal.ReplayStats) {
	t.Helper()

	out := []journal.Record{}
	stats, err := j.Replay(journal.Position{Segment: 1}, func(recs []journal.Record) error {
		for _, r := range recs {
			r.Value = append([]byte(nil), r.Value...)
			out = append(out, r)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}

	return out, stats
}

func TestAppendReplayAcrossSegments(t *testing.T) {
	dir := t.TempDir()
	j := openJournal(t, dir, journal.Options{Mode: journal.ModeWrite, SegmentSize: 4096})
	j.Start()

	locs := []journal.Location{}
	for i := 0; i < 500; i += 5 {
		batch := []journal.Record{record(i), record(i + 1), record(i + 2), record(i + 3), record(i + 4)}
		ticket, err := j.Append(batch)
		if err != nil {
			t.Fatal(err)
		}
		if err := ticket.Wait(); err != nil {
			t.Fatal(err)
		}
		for _, r := range batch {
			locs = append(locs, r.Loc)
		}
	}

	for i, loc := range locs {
		v, err := j.ReadValue(loc)
		if err != nil || !bytes.Equal(v, record(i).Value) {
			t.Fatalf("value %d: %q %v", i, v, err)
		}
	}

	if err := j.Close(); err != nil {
		t.Fatal(err)
	}

	segments, _ := j.Segments()
	if len(segments) < 5 {
		t.Fatalf("expected the log to rotate, got %d segments", len(segments))
	}

	j = openJournal(t, dir, journal.Options{Mode: journal.ModeWrite, SegmentSize: 4096})
	recs, stats := replayAll(t, j)
	if len(recs) != 500 || stats.Batches != 100 {
		t.Fatalf("replayed %d records in %d batches", len(recs), stats.Batches)
	}

	for i, r := range recs {
		if r.Key != record(i).Key || !bytes.Equal(r.Value, record(i).Value) || r.LSN != uint64(i+1) || r.Loc != locs[i] {
			t.Fatalf("record %d differs: %+v", i, r)
		}
	}

	if j.NextLSN() != 501 {
		t.Fatalf("next LSN %d", j.NextLSN())
	}

	j.Start()
	ticket, err := j.Append([]journal.Record{record(500)})
	if err != nil || ticket.Wait() != nil {
		t.Fatal(err)
	}
	if err := j.Close(); err != nil {
		t.Fatal(err)
	}

	j = openJournal(t, dir, journal.Options{SegmentSize: 4096})
	recs, _ = replayAll(t, j)
	if len(recs) != 501 || recs[500].LSN != 501 {
		t.Fatalf("appending after a replay continues the log, got %d records", len(recs))
	}
	j.Abandon()
}

func TestTornLastBatchIsDropped(t *testing.T) {
	dir := t.TempDir()
	j := openJournal(t, dir, journal.Options{Mode: journal.ModeWrite})
	j.Start()
	for i := 0; i < 3; i++ {
		ticket, _ := j.Append([]journal.Record{record(i)})
		_ = ticket.Wait()
	}
	_ = j.Close()

	path := filepath.Join(dir, journal.SegmentName(1))
	data, _ := os.ReadFile(path)
	full := len(data)

	for _, cut := range []int{1, 5, 17, 20} {
		if err := os.WriteFile(path, data[:full-cut], 0o600); err != nil {
			t.Fatal(err)
		}

		j = openJournal(t, dir, journal.Options{})
		recs, stats := replayAll(t, j)
		j.Abandon()
		if len(recs) != 2 || stats.Truncated == 0 {
			t.Fatalf("cut %d: expected 2 records and a truncation, got %d %+v", cut, len(recs), stats)
		}
	}

	garbage := append(append([]byte(nil), data...), []byte("not a batch at all")...)
	_ = os.WriteFile(path, garbage, 0o600)
	j = openJournal(t, dir, journal.Options{})
	recs, stats := replayAll(t, j)
	j.Abandon()
	if len(recs) != 3 || stats.Truncated != int64(len("not a batch at all")) {
		t.Fatalf("garbage after the last batch is dropped, got %d %+v", len(recs), stats)
	}
}

func TestCorruptionInASealedSegmentStopsTheReplay(t *testing.T) {
	dir := t.TempDir()
	j := openJournal(t, dir, journal.Options{Mode: journal.ModeWrite, SegmentSize: 256})
	j.Start()
	for i := 0; i < 20; i++ {
		ticket, _ := j.Append([]journal.Record{record(i)})
		_ = ticket.Wait()
	}
	_ = j.Close()

	path := filepath.Join(dir, journal.SegmentName(1))
	data, _ := os.ReadFile(path)
	data[len(data)-3] ^= 0xff
	_ = os.WriteFile(path, data, 0o600)

	j = openJournal(t, dir, journal.Options{})
	_, err := j.Replay(journal.Position{Segment: 1}, func([]journal.Record) error { return nil })
	j.Abandon()
	if !errors.Is(err, journal.ErrCorrupt) {
		t.Fatalf("expected a corruption error, got %v", err)
	}
}

type faultyFile struct {
	journal.File
	fs *faultyFS
}

func (f faultyFile) WriteAt(p []byte, off int64) (int, error) {
	switch f.fs.mode.Load() {
	case 1:
		return 0, syscall.ENOSPC
	case 2:
		n, _ := f.File.WriteAt(p[:len(p)/2], off)
		return n, nil
	}

	return f.File.WriteAt(p, off)
}

func (f faultyFile) Sync() error {
	if f.fs.mode.Load() == 3 {
		return errors.New("sync failed")
	}

	return f.File.Sync()
}

type faultyFS struct {
	mode atomic.Int32
}

func (fs *faultyFS) OpenFile(name string, flag int, perm os.FileMode) (journal.File, error) {
	f, err := journal.OSFS.OpenFile(name, flag, perm)
	if err != nil {
		return nil, err
	}

	return faultyFile{File: f, fs: fs}, nil
}

func (fs *faultyFS) Remove(name string) error                   { return journal.OSFS.Remove(name) }
func (fs *faultyFS) ReadDir(name string) ([]os.DirEntry, error) { return journal.OSFS.ReadDir(name) }
func (fs *faultyFS) MkdirAll(name string, perm os.FileMode) error {
	return journal.OSFS.MkdirAll(name, perm)
}

func TestWriteFaultsAreReportedAndRetried(t *testing.T) {
	for name, mode := range map[string]int32{"no space": 1, "short write": 2, "failed sync": 3} {
		t.Run(name, func(t *testing.T) {
			dir := t.TempDir()
			fs := &faultyFS{}
			var reported atomic.Int32
			j := openJournal(t, dir, journal.Options{Mode: journal.ModeSync, FS: fs, OnWriteError: func(error) { reported.Add(1) }})
			j.Start()

			ticket, _ := j.Append([]journal.Record{record(0)})
			if err := ticket.Wait(); err != nil {
				t.Fatal(err)
			}

			fs.mode.Store(mode)
			batch := []journal.Record{record(1)}
			ticket, _ = j.Append(batch)
			if err := ticket.Wait(); err == nil {
				t.Fatalf("the fault must reach the writer")
			}
			if reported.Load() == 0 {
				t.Fatalf("the fault must be reported")
			}

			if v, err := j.ReadValue(batch[0].Loc); err != nil || !bytes.Equal(v, record(1).Value) {
				t.Fatalf("an unwritten value stays readable: %q %v", v, err)
			}

			fs.mode.Store(0)
			if err := j.Flush(); err != nil {
				t.Fatalf("the write is retried once the disk works: %v", err)
			}
			if err := j.Close(); err != nil {
				t.Fatal(err)
			}

			j = openJournal(t, dir, journal.Options{})
			recs, _ := replayAll(t, j)
			j.Abandon()
			if len(recs) != 2 || recs[1].Key != record(1).Key {
				t.Fatalf("both records reach the disk, got %d", len(recs))
			}
		})
	}
}

func TestGroupCommitUnderConcurrentWriters(t *testing.T) {
	dir := t.TempDir()
	j := openJournal(t, dir, journal.Options{Mode: journal.ModeWrite})
	j.Start()

	var wg sync.WaitGroup
	for g := 0; g < 32; g++ {
		wg.Add(1)
		go func(g int) {
			defer wg.Done()
			for i := 0; i < 100; i++ {
				batch := []journal.Record{record(g*1000 + i)}
				ticket, err := j.Append(batch)
				if err != nil {
					t.Error(err)
					return
				}
				if err := ticket.Wait(); err != nil {
					t.Error(err)
					return
				}
				if v, err := j.ReadValue(batch[0].Loc); err != nil || !bytes.Equal(v, batch[0].Value) {
					t.Errorf("read back %q %v", v, err)
					return
				}
			}
		}(g)
	}
	wg.Wait()
	_ = j.Close()

	j = openJournal(t, dir, journal.Options{})
	recs, stats := replayAll(t, j)
	j.Abandon()
	if len(recs) != 3200 || stats.Batches != 3200 {
		t.Fatalf("replayed %d records", len(recs))
	}

	for i, r := range recs {
		if r.LSN != uint64(i+1) {
			t.Fatalf("LSNs follow the log order, record %d has %d", i, r.LSN)
		}
	}
}

func TestBackgroundModeWritesOnFlush(t *testing.T) {
	dir := t.TempDir()
	j := openJournal(t, dir, journal.Options{Mode: journal.ModeBackground})
	j.Start()

	batch := []journal.Record{record(0)}
	ticket, _ := j.Append(batch)
	if err := ticket.Wait(); err != nil {
		t.Fatal(err)
	}

	if j.PendingBytes() == 0 {
		t.Fatalf("background mode keeps the batch in memory until a flush")
	}

	if v, err := j.ReadValue(batch[0].Loc); err != nil || !bytes.Equal(v, record(0).Value) {
		t.Fatalf("a pending value is readable: %q %v", v, err)
	}

	if err := j.Flush(); err != nil {
		t.Fatal(err)
	}
	if j.PendingBytes() != 0 {
		t.Fatalf("a flush writes everything")
	}
	_ = j.Close()
}

func FuzzReplayNeverPanics(f *testing.F) {
	dir := f.TempDir()
	j, _ := journal.Open(dir, journal.Options{Mode: journal.ModeWrite})
	j.Start()
	ticket, _ := j.Append([]journal.Record{record(1), record(2)})
	_ = ticket.Wait()
	_ = j.Close()
	seed, _ := os.ReadFile(filepath.Join(dir, journal.SegmentName(1)))
	f.Add(seed)
	f.Add(seed[:20])

	f.Fuzz(func(t *testing.T, data []byte) {
		d := t.TempDir()
		_ = os.MkdirAll(d, 0o700)
		_ = os.WriteFile(filepath.Join(d, journal.SegmentName(1)), data, 0o600)
		j, err := journal.Open(d, journal.Options{})
		if err != nil {
			return
		}
		_, _ = j.Replay(journal.Position{Segment: 1}, func([]journal.Record) error { return nil })
		j.Abandon()
	})
}

func TestAbandonedJournalNeverWritesItsPendingBatches(t *testing.T) {
	dir := t.TempDir()
	j := openJournal(t, dir, journal.Options{Mode: journal.ModeWrite})
	j.Start()

	ticket, _ := j.Append([]journal.Record{record(0)})
	if err := ticket.Wait(); err != nil {
		t.Fatal(err)
	}

	background := openJournal(t, t.TempDir(), journal.Options{Mode: journal.ModeBackground})
	background.Start()
	if _, err := background.Append([]journal.Record{record(1)}); err != nil {
		t.Fatal(err)
	}
	background.Abandon()

	if err := background.Flush(); err == nil {
		t.Fatalf("an abandoned journal must refuse to flush")
	}

	if _, err := background.Append([]journal.Record{record(2)}); !errors.Is(err, journal.ErrClosed) {
		t.Fatalf("an abandoned journal must refuse new batches, got %v", err)
	}

	if segments, _ := background.Segments(); len(segments) != 0 {
		t.Fatalf("an abandoned journal must not write its pending batch, got %v", segments)
	}

	j.Abandon()

	reopened := openJournal(t, dir, journal.Options{Mode: journal.ModeWrite})
	recs, _ := replayAll(t, reopened)
	if len(recs) != 1 {
		t.Fatalf("expected the written record only, got %d", len(recs))
	}

	reopened.Start()
	for i := 1; i < 50; i++ {
		ticket, err := reopened.Append([]journal.Record{record(i)})
		if err != nil {
			t.Fatal(err)
		}
		if err := ticket.Wait(); err != nil {
			t.Fatal(err)
		}
	}
	if err := reopened.Close(); err != nil {
		t.Fatal(err)
	}

	final := openJournal(t, dir, journal.Options{})
	recs, stats := replayAll(t, final)
	final.Abandon()
	if len(recs) != 50 || stats.Truncated != 0 {
		t.Fatalf("the log written after a reopening is intact, got %d records and %d truncated bytes", len(recs), stats.Truncated)
	}
}
