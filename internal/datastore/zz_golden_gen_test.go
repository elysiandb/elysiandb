package datastore

import (
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"hash/crc32"
	"io/fs"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"testing"

	xxhash "github.com/cespare/xxhash/v2"
	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func goldenRoot(t *testing.T) string {
	t.Helper()
	root := os.Getenv("ELYSIANDB_GOLDEN_OUT")
	if root == "" {
		t.Skip("ELYSIANDB_GOLDEN_OUT is not set")
	}
	abs, err := filepath.Abs(root)
	if err != nil {
		t.Fatal(err)
	}
	if filepath.Base(abs) != "golden" || filepath.Base(filepath.Dir(abs)) != "tests" {
		t.Fatalf("ELYSIANDB_GOLDEN_OUT must name tests/golden, got %s", root)
	}
	return abs
}

func goldenSet(t *testing.T, set string) string {
	t.Helper()
	dir := filepath.Join(goldenRoot(t), "persistence", filepath.FromSlash(set))
	if err := os.RemoveAll(dir); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	return dir
}

func goldenFile(t *testing.T, rel string) string {
	t.Helper()
	path := filepath.Join(goldenRoot(t), "persistence", filepath.FromSlash(rel))
	if err := os.RemoveAll(path); err != nil {
		t.Fatal(err)
	}
	return path
}

func goldenJSON(t *testing.T, path string, v any) {
	t.Helper()
	data, err := goldenio.MarshalIndent(v)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatal(err)
	}
}

func formulaBytes(n, shift int) []byte {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte(((i+shift)*31 + 7) & 0xff)
	}
	return b
}

func hexOf(b []byte) string {
	return hex.EncodeToString(b)
}

func dec64(v uint64) string {
	return strconv.FormatUint(v, 10)
}

func decI64(v int64) string {
	return strconv.FormatInt(v, 10)
}

func copyTree(t *testing.T, src, dst string) {
	t.Helper()
	err := filepath.WalkDir(src, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(src, path)
		if err != nil {
			return err
		}
		target := filepath.Join(dst, rel)
		if d.IsDir() {
			return os.MkdirAll(target, 0o755)
		}
		if d.Name() == "-" {
			return nil
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		return os.WriteFile(target, data, 0o644)
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestGenerateGoldenVarints(t *testing.T) {
	path := goldenFile(t, "primitives/varint.json")
	uvals := []uint64{0, 1, 127, 128, 255, 16383, 16384, 1<<32 - 1, 1 << 35, 1 << 56, 1<<63 - 1, 1 << 63, math.MaxUint64}
	svals := []int64{0, 1, -1, 63, -63, 64, -64, -3, 1700000000, math.MinInt64, math.MaxInt64}
	var uv, sv, dv []map[string]any
	for _, v := range uvals {
		uv = append(uv, map[string]any{"value": dec64(v), "hex": hexOf(binary.AppendUvarint(nil, v))})
	}
	for _, v := range svals {
		sv = append(sv, map[string]any{"value": decI64(v), "hex": hexOf(binary.AppendVarint(nil, v))})
	}
	ff := func(n int, last byte) []byte {
		b := make([]byte, n+1)
		for i := 0; i < n; i++ {
			b[i] = 0xff
		}
		b[n] = last
		return b
	}
	bufs := [][]byte{{0x80, 0x00}, {0x80}, {}, ff(10, 0x01), ff(9, 0x02), ff(9, 0x01), ff(9, 0x00), {0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01}, {0x7f, 0x01}, {0x81, 0x01}, {0xfe, 0xff, 0x7f}}
	for _, b := range bufs {
		u, n := binary.Uvarint(b)
		dv = append(dv, map[string]any{"hex": hexOf(b), "signed": false, "value": dec64(u), "n": n})
		s, m := binary.Varint(b)
		dv = append(dv, map[string]any{"hex": hexOf(b), "signed": true, "value": decI64(s), "n": m})
	}
	for _, v := range uvals {
		b := binary.AppendUvarint(nil, v)
		u, n := binary.Uvarint(b)
		dv = append(dv, map[string]any{"hex": hexOf(b), "signed": false, "value": dec64(u), "n": n})
	}
	goldenJSON(t, path, map[string]any{"uvarint": uv, "varint": sv, "decode": dv})
}

func TestGenerateGoldenCrc32c(t *testing.T) {
	path := goldenFile(t, "primitives/crc32c.json")
	table := crc32.MakeTable(crc32.Castagnoli)
	var cases []map[string]any
	var lens []int
	for n := 0; n <= 64; n++ {
		lens = append(lens, n)
	}
	lens = append(lens, 255, 256, 1000, 4096, 65536)
	for _, n := range lens {
		b := formulaBytes(n, 0)
		cases = append(cases, map[string]any{"len": n, "formula": "(i*31+7)&0xff", "crc": crc32.Checksum(b, table)})
	}
	check := []byte("123456789")
	cases = append(cases, map[string]any{"len": len(check), "input": string(check), "crc": crc32.Checksum(check, table)})
	b := formulaBytes(4096, 0)
	h := crc32.New(table)
	parts := [][2]int{{0, 1}, {1, 1000}, {1000, 4096}}
	for _, p := range parts {
		_, _ = h.Write(b[p[0]:p[1]])
	}
	incremental := map[string]any{"len": len(b), "formula": "(i*31+7)&0xff", "slices": parts, "crc": h.Sum32(), "update": crc32.Update(crc32.Checksum(b[:1000], table), table, b[1000:])}
	goldenJSON(t, path, map[string]any{"cases": cases, "incremental": incremental})
}

func TestGenerateGoldenXxh64(t *testing.T) {
	path := goldenFile(t, "primitives/xxh64.json")
	var cases []map[string]any
	add := func(s string) {
		sum := xxhash.Sum64String(s)
		cases = append(cases, map[string]any{"hex": hexOf([]byte(s)), "sum": dec64(sum), "shard64": sum & 63, "shard8": sum & 7})
	}
	for n := 0; n <= 300; n++ {
		add(string(formulaBytes(n, 0)))
	}
	for _, s := range []string{"", "a", "abc", "session:1", "key-00000001", "\xff\xfe"} {
		add(s)
	}
	goldenJSON(t, path, map[string]any{"cases": cases})
}

type scriptRecord struct {
	Kind      int    `json:"kind"`
	Gen       string `json:"gen"`
	ExpiresAt string `json:"expires_at"`
	Entity    string `json:"entity_hex"`
	Key       string `json:"key_hex"`
	Value     string `json:"value_hex"`
}

func journalScript() [][]journal.Record {
	rec := func(kind journal.Kind, gen uint64, exp int64, entity, key string, value []byte) journal.Record {
		return journal.Record{Kind: kind, Gen: gen, ExpiresAt: exp, Entity: entity, Key: key, Value: value}
	}
	var batches [][]journal.Record
	batches = append(batches, []journal.Record{rec(journal.KindDocPut, 0, 0, "book", "b1", formulaBytes(40, 0))})
	batches = append(batches, []journal.Record{
		rec(journal.KindDocDelete, 0, 0, "book", "b1", nil),
		rec(journal.KindDocMove, 3, 0, "book", "b2", formulaBytes(12, 5)),
		rec(journal.KindEntityReset, 1<<40, 0, "book", "", nil),
		rec(journal.KindEntityCreate, 0, 0, "a\u2028b", "", nil),
		rec(journal.KindEntityDrop, 2, 0, "old", "", nil),
	})
	batches = append(batches, []journal.Record{
		rec(journal.KindKVPut, 0, 1<<40, "", "k", formulaBytes(300, 1)),
		rec(journal.KindKVDelete, 0, 0, "", "\xff\xfe", nil),
		rec(journal.KindKVExpire, 0, -1, "", "k", nil),
		rec(journal.KindKVMove, 0, 0, "", "k2", formulaBytes(7, 2)),
		rec(journal.KindKVReset, 0, 0, "", "", nil),
	})
	batches = append(batches, []journal.Record{rec(journal.KindClearAll, 0, 0, "", "", nil)})
	var big []journal.Record
	for i := 0; i < 64; i++ {
		big = append(big, rec(journal.KindDocPut, uint64(i), int64(i)-32, "e", fmt.Sprintf("key-%08d", i), formulaBytes(i, i)))
	}
	batches = append(batches, big)
	batches = append(batches, []journal.Record{rec(journal.KindDocPut, 0, 0, "big", "v", formulaBytes(10<<10, 3))})
	batches = append(batches, []journal.Record{rec(journal.KindDocPut, 0, 0, "", "", nil)})
	var five []journal.Record
	for i := 0; i < 5; i++ {
		five = append(five, rec(journal.KindKVPut, 0, 0, "", fmt.Sprintf("key-%08d", i), formulaBytes(20, i)))
	}
	batches = append(batches, five)
	batches = append(batches, []journal.Record{rec(journal.KindDocPut, 0, 0, "book", "late", formulaBytes(300, 9))})
	return batches
}

func describeScript(batches [][]journal.Record) [][]scriptRecord {
	out := make([][]scriptRecord, len(batches))
	for i, b := range batches {
		for _, r := range b {
			out[i] = append(out[i], scriptRecord{Kind: int(r.Kind), Gen: dec64(r.Gen), ExpiresAt: decI64(r.ExpiresAt), Entity: hexOf([]byte(r.Entity)), Key: hexOf([]byte(r.Key)), Value: hexOf(r.Value)})
		}
	}
	return out
}

func segmentList(t *testing.T, j *journal.Journal) []map[string]any {
	t.Helper()
	segs, err := j.Segments()
	if err != nil {
		t.Fatal(err)
	}
	var out []map[string]any
	for _, s := range segs {
		out = append(out, map[string]any{"id": s.ID, "size": s.Size})
	}
	return out
}

func writeJournalLog(t *testing.T, logDir string) map[string]any {
	t.Helper()
	j, err := journal.Open(logDir, journal.Options{Mode: journal.ModeSync, SegmentSize: 4096})
	if err != nil {
		t.Fatal(err)
	}
	j.Start()
	var records []map[string]any
	for bi, batch := range journalScript() {
		recs := append([]journal.Record(nil), batch...)
		ticket, err := j.Append(recs)
		if err != nil {
			t.Fatal(err)
		}
		if err := ticket.Wait(); err != nil {
			t.Fatal(err)
		}
		for ri, r := range recs {
			records = append(records, map[string]any{"batch": bi, "record": ri, "lsn": dec64(r.LSN), "segment": r.Loc.Segment, "offset": r.Loc.Offset, "length": r.Loc.Length})
		}
	}
	next := j.NextLSN()
	pos := j.Position()
	segs := segmentList(t, j)
	if err := j.Close(); err != nil {
		t.Fatal(err)
	}
	return map[string]any{"records": records, "next_lsn": dec64(next), "position": map[string]any{"segment": pos.Segment, "offset": pos.Offset}, "segments": segs}
}

func TestGenerateGoldenJournal(t *testing.T) {
	dir := goldenSet(t, "journal/encode")
	goldenJSON(t, filepath.Join(dir, "script.json"), map[string]any{"options": map[string]any{"mode": "sync", "segment_size": 4096}, "batches": describeScript(journalScript())})
	logDir := filepath.Join(t.TempDir(), "log")
	expected := writeJournalLog(t, logDir)
	copyTree(t, logDir, filepath.Join(dir, "log"))
	goldenJSON(t, filepath.Join(dir, "expected.json"), expected)
}

func TestGenerateGoldenJournalDamage(t *testing.T) {
	dir := goldenSet(t, "journal/damage")
	base := filepath.Join(t.TempDir(), "log")
	writeJournalLog(t, base)
	segs, err := os.ReadDir(base)
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, e := range segs {
		names = append(names, e.Name())
	}
	sort.Strings(names)
	if len(names) < 3 {
		t.Fatalf("expected at least 3 segments, got %v", names)
	}
	first := names[0]
	second := names[1]
	last := names[len(names)-1]
	mutate := func(name string, fn func([]byte) []byte) func(string) {
		return func(d string) {
			p := filepath.Join(d, name)
			data, err := os.ReadFile(p)
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(p, fn(data), 0o644); err != nil {
				t.Fatal(err)
			}
		}
	}
	cut := func(n int) func([]byte) []byte {
		return func(b []byte) []byte { return b[:len(b)-n] }
	}
	lastID, _ := strconv.Atoi(last[:6])
	cases := []struct {
		name string
		do   func(string)
	}{
		{"cut-1", mutate(last, cut(1))},
		{"cut-5", mutate(last, cut(5))},
		{"cut-17", mutate(last, cut(17))},
		{"cut-20", mutate(last, cut(20))},
		{"garbage-tail", mutate(last, func(b []byte) []byte { return append(b, []byte("not a batch at all")...) })},
		{"flip-mid-first", mutate(first, func(b []byte) []byte { b[len(b)/2] ^= 0x40; return b })},
		{"zero-tail", mutate(last, func(b []byte) []byte { return append(b, make([]byte, 512)...) })},
		{"zeroed-last-bytes", mutate(last, func(b []byte) []byte {
			for i := len(b) - 32; i < len(b); i++ {
				b[i] = 0
			}
			return b
		})},
		{"wrong-header-id", mutate(second, func(b []byte) []byte { binary.LittleEndian.PutUint32(b[8:], 99); return b })},
		{"ten-byte-file", func(d string) {
			if err := os.WriteFile(filepath.Join(d, journal.SegmentName(uint32(lastID+1))), []byte("ELYSLOG1\x01\x00"), 0o644); err != nil {
				t.Fatal(err)
			}
		}},
		{"intact", func(string) {}},
	}
	for _, c := range cases {
		work := filepath.Join(t.TempDir(), "log")
		copyTree(t, base, work)
		c.do(work)
		copyTree(t, work, filepath.Join(dir, c.name, "before"))
		j, err := journal.Open(work, journal.Options{Mode: journal.ModeSync, SegmentSize: 4096})
		if err != nil {
			t.Fatal(err)
		}
		applied := []string{}
		stats, err := j.Replay(journal.Position{Segment: 1}, func(recs []journal.Record) error {
			for _, r := range recs {
				applied = append(applied, dec64(r.LSN))
			}
			return nil
		})
		if err != nil {
			t.Fatalf("%s: %v", c.name, err)
		}
		next := j.NextLSN()
		pos := j.Position()
		if err := j.Close(); err != nil {
			t.Fatalf("%s: %v", c.name, err)
		}
		copyTree(t, work, filepath.Join(dir, c.name, "after"))
		goldenJSON(t, filepath.Join(dir, c.name, "stats.json"), map[string]any{
			"set_aside": nonNil(stats.SetAside), "damaged": nonNil(stats.Damaged), "batches": stats.Batches, "records": stats.Records,
			"truncated": stats.Truncated, "last_lsn": dec64(stats.LastLSN), "applied": applied, "next_lsn": dec64(next),
			"position": map[string]any{"segment": pos.Segment, "offset": pos.Offset},
		})
	}
}

func nonNil(v []uint32) []uint32 {
	if v == nil {
		return []uint32{}
	}
	return v
}
