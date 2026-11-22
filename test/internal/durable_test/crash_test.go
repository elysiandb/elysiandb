package durable_test

import (
	"encoding/json"
	"fmt"
	"testing"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/journal"
)

func engineOptions(dir string, fs *memFS) datastore.Options {
	return datastore.Options{
		Folder:          dir,
		Shards:          4,
		Durability:      journal.ModeSync,
		SegmentSize:     4 << 10,
		PageCacheBytes:  8 << 10,
		PageSegmentSize: 16 << 10,
		CacheBytes:      4 << 10,
		FS:              fs,
	}
}

type ledger struct {
	docs    map[string]string
	kv      map[string]string
	pending map[string]string
}

func newLedger() *ledger {
	return &ledger{docs: map[string]string{}, kv: map[string]string{}, pending: map[string]string{}}
}

const deleted = "\x00deleted"

func runWorkload(e *datastore.Engine, l *ledger, steps int) {
	for i := 0; i < steps; i++ {
		id := fmt.Sprintf("d%02d", (i*7)%23)
		value := fmt.Sprintf("v%d-%s", i, id)

		if i%5 == 3 {
			l.pending[id] = deleted
			if err := e.Update(func(tx *datastore.Tx) error {
				tx.Delete("a", id)
				return nil
			}); err != nil {
				return
			}
			l.docs[id] = deleted
			delete(l.pending, id)
		} else {
			l.pending[id] = value
			if err := e.Update(func(tx *datastore.Tx) error {
				tx.EnsureType("a")
				return tx.Put("a", id, map[string]any{"v": value, "pad": fmt.Sprintf("%0120d", i)})
			}); err != nil {
				return
			}
			l.docs[id] = value
			delete(l.pending, id)
		}

		if i%4 == 0 {
			key := fmt.Sprintf("k%d", i%9)
			l.pending["kv:"+key] = value
			if err := e.KVPut(key, []byte(value), 0); err != nil {
				return
			}
			l.kv[key] = value
			delete(l.pending, "kv:"+key)
		}

		if i%13 == 12 {
			_ = e.Checkpoint()
		}

		if i%29 == 28 {
			_, _ = e.Compact(2)
		}
	}
}

func checkLedger(t *testing.T, label string, e *datastore.Engine, l *ledger) {
	t.Helper()

	snap := e.Snapshot("a")
	for id, want := range l.docs {
		allowed := map[string]bool{want: true}
		if p, ok := l.pending[id]; ok {
			allowed[p] = true
		}

		got := deleted
		if d, ok := snap.Get(id); ok {
			doc, err := d.Decode()
			if err != nil {
				t.Fatalf("%s: document %s unreadable: %v", label, id, err)
			}
			got, _ = doc["v"].(string)
		}

		if !allowed[got] {
			t.Fatalf("%s: document %s is %q, acknowledged %q", label, id, got, want)
		}
	}

	for key, want := range l.kv {
		allowed := map[string]bool{want: true}
		if p, ok := l.pending["kv:"+key]; ok {
			allowed[p] = true
		}

		v, _, ok := e.KVGet(key)
		if !ok || !allowed[string(v)] {
			t.Fatalf("%s: key %s is %q (%v), acknowledged %q", label, key, v, ok, want)
		}
	}
}

func crashRun(t *testing.T, dir string, crashAt, steps int) (*memFS, *ledger) {
	t.Helper()

	fs := newMemFS()
	fs.crashAt = crashAt
	e, err := datastore.Open(engineOptions(dir, fs))
	if err != nil {
		if crashAt > 0 {
			return fs, newLedger()
		}
		t.Fatal(err)
	}

	l := newLedger()
	done := make(chan struct{})
	go func() {
		defer close(done)
		runWorkload(e, l, steps)
	}()

	select {
	case <-done:
		e.Abandon()
	case <-fs.down:
		e.Abandon()
		<-done
	}

	return fs, l
}

func (l *ledger) clone() *ledger {
	c := newLedger()
	for k, v := range l.docs {
		c.docs[k] = v
	}
	for k, v := range l.kv {
		c.kv[k] = v
	}
	for k, v := range l.pending {
		c.pending[k] = v
	}

	return c
}

func reopenAndCheck(t *testing.T, label, dir string, image *memFS, acked *ledger) {
	t.Helper()

	l := acked.clone()

	e, err := datastore.Open(engineOptions(dir, image))
	if err != nil {
		t.Fatalf("%s: cannot start after the crash: %v", label, err)
	}

	checkLedger(t, label, e, l)

	if err := e.Update(func(tx *datastore.Tx) error {
		tx.EnsureType("a")
		return tx.Put("a", "after", map[string]any{"v": "after"})
	}); err != nil {
		t.Fatalf("%s: cannot write after recovery: %v", label, err)
	}
	if err := e.Checkpoint(); err != nil {
		t.Fatalf("%s: cannot checkpoint after recovery: %v", label, err)
	}
	if err := e.Close(); err != nil {
		t.Fatalf("%s: cannot close after recovery: %v", label, err)
	}

	e, err = datastore.Open(engineOptions(dir, image))
	if err != nil {
		t.Fatalf("%s: cannot start a second time: %v", label, err)
	}
	l.docs["after"] = "after"
	checkLedger(t, label+" (second start)", e, l)
	e.Abandon()
}

func TestEveryCrashPointKeepsAcknowledgedWrites(t *testing.T) {
	const steps = 70
	dir := t.TempDir()

	clean, _ := crashRun(t, dir, 0, steps)
	total := clean.Ops()
	if total < 200 {
		t.Fatalf("the workload should touch the disk many times, got %d operations", total)
	}

	for crashAt := 1; crashAt <= total; crashAt++ {
		fs, l := crashRun(t, dir, crashAt, steps)
		reopenAndCheck(t, fmt.Sprintf("power cut at operation %d/%d", crashAt, total), dir, fs.afterPowerLoss(), l)
		reopenAndCheck(t, fmt.Sprintf("process crash at operation %d/%d", crashAt, total), dir, fs.afterProcessCrash(), l)
	}
}

func TestStateSurvivesRepeatedRestarts(t *testing.T) {
	dir := t.TempDir()
	fs := newMemFS()
	l := newLedger()

	for round := 0; round < 40; round++ {
		e, err := datastore.Open(engineOptions(dir, fs))
		if err != nil {
			t.Fatalf("round %d: %v", round, err)
		}
		checkLedger(t, fmt.Sprintf("round %d", round), e, l)

		for i := 0; i < 15; i++ {
			id := fmt.Sprintf("r%03d", (round*15+i)%97)
			value := fmt.Sprintf("round%d-%d", round, i)
			if err := e.Update(func(tx *datastore.Tx) error {
				tx.EnsureType("a")
				return tx.Put("a", id, map[string]any{"v": value})
			}); err != nil {
				t.Fatal(err)
			}
			l.docs[id] = value
		}

		switch round % 3 {
		case 0:
			_ = e.Close()
		case 1:
			_ = e.Checkpoint()
			e.Abandon()
			fs = fs.afterPowerLoss()
		default:
			e.Abandon()
			fs = fs.afterProcessCrash()
		}
	}

	if files := len(fs.list(dir)); files > 120 {
		data, _ := json.Marshal(fs.list(dir))
		t.Fatalf("repeated restarts should not pile up files, %d left: %s", files, data)
	}
}
