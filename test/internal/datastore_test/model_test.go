package datastore_test

import (
	"bufio"
	"encoding/json"
	"fmt"
	"math/rand"
	"os"
	"os/exec"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/journal"
)

type model struct {
	types []string
	docs  map[string][]string
	data  map[string]map[string]string
	kv    map[string]string
}

func newModel() *model {
	return &model{docs: map[string][]string{}, data: map[string]map[string]string{}, kv: map[string]string{}}
}

func (m *model) ensureType(entity string) {
	for _, t := range m.types {
		if t == entity {
			return
		}
	}
	m.types = append(m.types, entity)
}

func (m *model) put(entity, id, value string) {
	if m.data[entity] == nil {
		m.data[entity] = map[string]string{}
	}

	if _, ok := m.data[entity][id]; !ok {
		m.docs[entity] = append(m.docs[entity], id)
	}
	m.data[entity][id] = value
}

func (m *model) del(entity, id string) {
	if _, ok := m.data[entity][id]; !ok {
		return
	}

	delete(m.data[entity], id)
	kept := []string{}
	for _, d := range m.docs[entity] {
		if d != id {
			kept = append(kept, d)
		}
	}
	m.docs[entity] = kept
}

func (m *model) reset(entity string) {
	delete(m.data, entity)
	delete(m.docs, entity)
}

func options(dir string) datastore.Options {
	return datastore.Options{Folder: dir, Shards: 8, Durability: journal.ModeWrite, SegmentSize: 8 << 10}
}

func compare(t *testing.T, step int, e *datastore.Engine, m *model) {
	t.Helper()

	if got := e.Types(); !reflect.DeepEqual(append([]string{}, got...), append([]string{}, m.types...)) {
		t.Fatalf("step %d: types %v, want %v", step, got, m.types)
	}

	for _, entity := range []string{"a", "b", "c"} {
		snap := e.Snapshot(entity)
		got := []string{}
		snap.Ascend(0, func(d datastore.Doc) bool {
			b, err := d.Bytes()
			if err != nil {
				t.Fatalf("step %d: %v", step, err)
			}
			var v map[string]string
			_ = json.Unmarshal(b, &v)
			if v["v"] != m.data[entity][d.ID()] {
				t.Fatalf("step %d: %s/%s = %q, want %q", step, entity, d.ID(), v["v"], m.data[entity][d.ID()])
			}
			got = append(got, d.ID())
			return true
		})

		want := m.docs[entity]
		if len(got) != len(want) || (len(want) > 0 && !reflect.DeepEqual(got, want)) {
			t.Fatalf("step %d: %s order %v, want %v", step, entity, got, want)
		}

		if snap.Len() != len(want) {
			t.Fatalf("step %d: %s count %d, want %d", step, entity, snap.Len(), len(want))
		}
	}

	keys := e.KVKeys()
	want := make([]string, 0, len(m.kv))
	for k := range m.kv {
		want = append(want, k)
	}
	sort.Strings(want)
	if len(keys) != len(want) || (len(want) > 0 && !reflect.DeepEqual(keys, want)) {
		t.Fatalf("step %d: kv keys %v, want %v", step, keys, want)
	}

	for k, v := range m.kv {
		got, _, ok := e.KVGet(k)
		if !ok || string(got) != v {
			t.Fatalf("step %d: kv %s = %q, want %q", step, k, got, v)
		}
	}
}

func TestRandomOperationsAgainstAModel(t *testing.T) {
	for _, c := range []struct{ cache, pages, segment int64 }{{0, 0, 0}, {4 << 10, 0, 0}, {4 << 10, 16 << 10, 64 << 10}} {
		t.Run(fmt.Sprintf("cache=%d/pages=%d", c.cache, c.pages), func(t *testing.T) {
			dir := t.TempDir()
			opts := options(dir)
			opts.CacheBytes = c.cache
			opts.PageCacheBytes = c.pages
			opts.PageSegmentSize = c.segment
			e, err := datastore.Open(opts)
			if err != nil {
				t.Fatal(err)
			}

			r := rand.New(rand.NewSource(11))
			m := newModel()
			entities := []string{"a", "b", "c"}

			for step := 0; step < 4000; step++ {
				entity := entities[r.Intn(len(entities))]
				id := fmt.Sprintf("d%02d", r.Intn(60))
				switch op := r.Intn(100); {
				case op < 45:
					value := fmt.Sprintf("%d-%s", step, strings.Repeat("x", r.Intn(300)))
					err = e.Update(func(tx *datastore.Tx) error {
						tx.EnsureType(entity)
						return tx.Put(entity, id, map[string]any{"v": value})
					})
					m.ensureType(entity)
					m.put(entity, id, value)
				case op < 65:
					err = e.Update(func(tx *datastore.Tx) error {
						tx.Delete(entity, id)
						return nil
					})
					m.del(entity, id)
				case op < 67:
					err = e.Update(func(tx *datastore.Tx) error {
						tx.ResetEntity(entity)
						return nil
					})
					m.reset(entity)
				case op < 80:
					key := fmt.Sprintf("k%02d", r.Intn(40))
					value := fmt.Sprintf("%d", step)
					err = e.KVPut(key, []byte(value), 0)
					m.kv[key] = value
				case op < 88:
					key := fmt.Sprintf("k%02d", r.Intn(40))
					e.KVDelete(key)
					delete(m.kv, key)
				case op < 92:
					if step%3 == 0 {
						_, err = e.CompactPages()
					} else {
						err = e.Checkpoint()
					}
				case op < 95:
					_, err = e.Compact(4)
				case op < 98:
					err = e.Close()
					if err == nil {
						e, err = datastore.Open(opts)
					}
				default:
					e.Abandon()
					e, err = datastore.Open(opts)
				}

				if err != nil {
					t.Fatalf("step %d: %v", step, err)
				}

				if step%250 == 0 {
					compare(t, step, e, m)
				}
			}

			compare(t, -1, e, m)
			if err := e.Close(); err != nil {
				t.Fatal(err)
			}

			e, err = datastore.Open(opts)
			if err != nil {
				t.Fatal(err)
			}
			compare(t, -2, e, m)

			stats := e.Stats()
			if stats.Segments == 0 {
				t.Fatalf("expected log segments")
			}
			_ = e.Close()
		})
	}
}

func TestCompactionRemovesDeadSegments(t *testing.T) {
	dir := t.TempDir()
	e, err := datastore.Open(options(dir))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	for round := 0; round < 30; round++ {
		if err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("a")
			for i := 0; i < 20; i++ {
				if err := tx.Put("a", fmt.Sprintf("d%02d", i), map[string]any{"v": strings.Repeat("y", 100), "round": round}); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}

	if err := e.KVPut("kept", []byte("value"), 0); err != nil {
		t.Fatal(err)
	}

	before := e.Stats()
	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}

	for i := 0; i < 10; i++ {
		if _, err := e.Compact(10); err != nil {
			t.Fatal(err)
		}
		if err := e.Checkpoint(); err != nil {
			t.Fatal(err)
		}
	}

	after := e.Stats()
	if after.LogBytes >= before.LogBytes/2 {
		t.Fatalf("compaction should reclaim most of the log: %d bytes before, %d after", before.LogBytes, after.LogBytes)
	}

	snap := e.Snapshot("a")
	if snap.Len() != 20 {
		t.Fatalf("documents survive compaction, got %d", snap.Len())
	}

	doc, _ := snap.Get("d05")
	data, _ := doc.Decode()
	if data["round"] != 29.0 {
		t.Fatalf("the latest version survives compaction, got %v", data)
	}

	if v, _, ok := e.KVGet("kept"); !ok || string(v) != "value" {
		t.Fatalf("kv keys survive compaction")
	}
}

func TestSnapshotsAreStableWhileWritesContinue(t *testing.T) {
	dir := t.TempDir()
	e, err := datastore.Open(options(dir))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	for i := 0; i < 100; i++ {
		_ = e.Update(func(tx *datastore.Tx) error {
			return tx.Put("a", fmt.Sprintf("d%03d", i), map[string]any{"v": i})
		})
	}

	snap := e.Snapshot("a")
	for i := 0; i < 50; i++ {
		_ = e.Update(func(tx *datastore.Tx) error {
			tx.Delete("a", fmt.Sprintf("d%03d", i))
			return tx.Put("a", fmt.Sprintf("n%03d", i), map[string]any{"v": i})
		})
	}

	if snap.Len() != 100 {
		t.Fatalf("an old snapshot keeps its documents, got %d", snap.Len())
	}

	if _, ok := snap.Get("d000"); !ok {
		t.Fatalf("an old snapshot still sees a deleted document")
	}

	if e.Snapshot("a").Len() != 100 {
		t.Fatalf("the new snapshot has 50 old and 50 new documents")
	}
}

func TestConflictingTransactionsAreRetried(t *testing.T) {
	dir := t.TempDir()
	e, err := datastore.Open(options(dir))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	_ = e.Update(func(tx *datastore.Tx) error {
		return tx.Put("a", "counter", map[string]any{"n": 0.0})
	})

	done := make(chan struct{})
	for g := 0; g < 8; g++ {
		go func() {
			defer func() { done <- struct{}{} }()
			for i := 0; i < 50; i++ {
				_ = e.Update(func(tx *datastore.Tx) error {
					doc := tx.Read("a", "counter")
					doc["n"] = doc["n"].(float64) + 1
					return tx.Put("a", "counter", doc)
				})
			}
		}()
	}
	for g := 0; g < 8; g++ {
		<-done
	}

	doc, _ := e.Snapshot("a").Get("counter")
	data, _ := doc.Decode()
	if data["n"] != 400.0 {
		t.Fatalf("read-modify-write transactions lose no update, got %v", data["n"])
	}
}

func TestLockFileRejectsASecondProcessOnTheFolder(t *testing.T) {
	dir := t.TempDir()
	e, err := datastore.Open(options(dir))
	if err != nil {
		t.Fatal(err)
	}

	if os.Getenv("ELYSIAN_LOCK_CHILD") == "" {
		cmd := exec.Command(os.Args[0], "-test.run", "^TestLockFileRejectsASecondProcessOnTheFolder$")
		cmd.Env = append(os.Environ(), "ELYSIAN_LOCK_CHILD="+dir)
		out, _ := cmd.CombinedOutput()
		if !strings.Contains(string(out), "LOCKED") {
			t.Fatalf("a second process must not open the folder: %s", out)
		}
	}

	_ = e.Close()
}

func TestMain(m *testing.M) {
	if dir := os.Getenv("ELYSIAN_LOCK_CHILD"); dir != "" {
		if _, err := datastore.Open(options(dir)); err != nil {
			fmt.Println("LOCKED", err)
			os.Exit(0)
		}
		fmt.Println("OPENED")
		os.Exit(0)
	}

	if dir := os.Getenv("ELYSIAN_CRASH_CHILD"); dir != "" {
		crashChild(dir)
		return
	}

	os.Exit(m.Run())
}

func crashChild(dir string) {
	e, err := datastore.Open(options(dir))
	if err != nil {
		fmt.Println("ERR", err)
		os.Exit(1)
	}

	w := bufio.NewWriter(os.Stdout)
	start := e.Snapshot("a").NextSeq()
	for i := 0; ; i++ {
		id := fmt.Sprintf("p%d-%d", start, i)
		err := e.Update(func(tx *datastore.Tx) error {
			tx.EnsureType("a")
			if i%7 == 3 {
				tx.Delete("a", fmt.Sprintf("p%d-%d", start, i-1))
			}
			return tx.Put("a", id, map[string]any{"i": i})
		})
		if err != nil {
			fmt.Println("ERR", err)
			os.Exit(1)
		}

		if err := e.KVPut("kv-"+id, []byte(strconv.Itoa(i)), 0); err != nil {
			os.Exit(1)
		}

		fmt.Fprintf(w, "ACK %s %d\n", id, i%7)
		_ = w.Flush()

		if i%200 == 199 {
			_ = e.Checkpoint()
		}
	}
}

func TestKill9LosesNoAcknowledgedWrite(t *testing.T) {
	runs := 10
	if v, err := strconv.Atoi(os.Getenv("ELYSIAN_KILL9_RUNS")); err == nil {
		runs = v
	}

	dir := t.TempDir()
	acked := map[string]bool{}
	deleted := map[string]bool{}
	r := rand.New(rand.NewSource(time.Now().UnixNano()))

	for run := 0; run < runs; run++ {
		cmd := exec.Command(os.Args[0], "-test.run", "^$")
		cmd.Env = append(os.Environ(), "ELYSIAN_CRASH_CHILD="+dir)
		stdout, _ := cmd.StdoutPipe()
		if err := cmd.Start(); err != nil {
			t.Fatal(err)
		}

		lines := make(chan string, 1024)
		go func() {
			s := bufio.NewScanner(stdout)
			for s.Scan() {
				lines <- s.Text()
			}
			close(lines)
		}()

		deadline := time.After(time.Duration(50+r.Intn(400)) * time.Millisecond)
		killed := false
		for !killed {
			select {
			case line, ok := <-lines:
				if !ok {
					killed = true
					break
				}
				var id string
				var mod int
				if n, _ := fmt.Sscanf(line, "ACK %s %d", &id, &mod); n == 2 {
					acked[id] = true
				}
			case <-deadline:
				_ = cmd.Process.Signal(syscall.SIGKILL)
				killed = true
			}
		}

		_ = cmd.Wait()
		for line := range lines {
			var id string
			var mod int
			if n, _ := fmt.Sscanf(line, "ACK %s %d", &id, &mod); n == 2 {
				acked[id] = true
			}
		}

		e, err := datastore.Open(options(dir))
		if err != nil {
			t.Fatalf("run %d: cannot reopen after kill -9: %v", run, err)
		}

		snap := e.Snapshot("a")
		present := map[string]bool{}
		snap.Ascend(0, func(d datastore.Doc) bool {
			present[d.ID()] = true
			return true
		})

		for id := range acked {
			var start, i int
			_, _ = fmt.Sscanf(id, "p%d-%d", &start, &i)
			next := fmt.Sprintf("p%d-%d", start, i+1)
			if (i+1)%7 == 3 && present[next] {
				deleted[id] = true
			}
		}

		for id := range acked {
			if deleted[id] {
				continue
			}

			if !present[id] {
				t.Fatalf("run %d: acknowledged document %s is missing after kill -9", run, id)
			}

			if _, _, ok := e.KVGet("kv-" + id); !ok {
				t.Fatalf("run %d: acknowledged key kv-%s is missing after kill -9", run, id)
			}
		}

		if snap.Len() != len(present) {
			t.Fatalf("run %d: count %d differs from the listed documents %d", run, snap.Len(), len(present))
		}

		if err := e.Close(); err != nil {
			t.Fatal(err)
		}
	}

	if len(acked) == 0 {
		t.Fatalf("the child wrote nothing")
	}
}
