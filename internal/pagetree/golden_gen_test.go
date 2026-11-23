package pagetree

import (
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

type memStore struct {
	next  uint64
	pages map[uint64][]byte
}

func (m *memStore) Load(ref *Ref) (*Page, error) {
	b, ok := m.pages[ref.ID()]
	if !ok {
		return nil, fmt.Errorf("page %d missing", ref.ID())
	}
	return Decode(b)
}

func (m *memStore) flush(t *testing.T, tr Tree) []map[string]any {
	t.Helper()
	written := []map[string]any{}
	err := tr.ForEachDirty(func(ref *Ref, p *Page) error {
		enc, err := Encode(nil, p)
		if err != nil {
			return err
		}
		m.next++
		m.pages[m.next] = enc
		ref.Persisted(m.next, uint32(len(enc)))
		written = append(written, map[string]any{"id": m.next, "size": len(enc), "bytes": hex.EncodeToString(enc)})
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return written
}

func fbytes(n, shift int) []byte {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte(((i+shift)*31 + 7) & 0xff)
	}
	return b
}

func treeResult(t *testing.T, store *memStore, name string, ops any, tr Tree, pages []map[string]any) map[string]any {
	t.Helper()
	row := map[string]any{"scenario": name, "ops": ops, "pages": pages, "len": tr.Len()}
	if r := tr.Root(); r != nil {
		row["root"] = map[string]any{"id": r.ID(), "size": r.Size()}
	} else {
		row["root"] = nil
	}
	dump := [][]string{}
	tr.Scan(func(k string, v []byte) bool {
		dump = append(dump, []string{hex.EncodeToString([]byte(k)), hex.EncodeToString(v)})
		return true
	})
	if len(dump) > 5000 {
		row["dump_head"] = dump[:50]
		row["dump_tail"] = dump[len(dump)-50:]
	} else {
		row["dump"] = dump
	}
	probes := map[string]any{}
	var ranks []any
	var ats []any
	n := tr.Len()
	for _, i := range []int{0, 1, n / 3, n / 2, n - 1, n, n + 5, -1} {
		k, v, ok := tr.At(i)
		ats = append(ats, map[string]any{"index": i, "ok": ok, "key": hex.EncodeToString([]byte(k)), "val": hex.EncodeToString(v)})
		if ok {
			ranks = append(ranks, map[string]any{"key": hex.EncodeToString([]byte(k)), "rank": tr.Rank(k)})
			ranks = append(ranks, map[string]any{"key": hex.EncodeToString([]byte(k + "\x00")), "rank": tr.Rank(k + "\x00")})
		}
	}
	ranks = append(ranks, map[string]any{"key": "", "rank": tr.Rank("")}, map[string]any{"key": "ffff", "rank": tr.Rank("\xff\xff")})
	probes["rank"] = ranks
	probes["at"] = ats
	row["probes"] = probes
	_ = store
	return row
}

type op struct {
	Op  string `json:"op"`
	Key string `json:"key"`
	Val string `json:"val,omitempty"`
}

func runOps(tr Tree, ops []op) Tree {
	for _, o := range ops {
		k, _ := hex.DecodeString(o.Key)
		v, _ := hex.DecodeString(o.Val)
		switch o.Op {
		case "put":
			tr, _ = tr.Put(string(k), v)
		case "delete":
			tr, _ = tr.Delete(string(k))
		case "upsert":
			tr = tr.Upsert(string(k), func(cur []byte, found bool) []byte {
				if found {
					return append(append([]byte(nil), cur...), v...)
				}
				return v
			})
		case "update":
			tr, _ = tr.Update(string(k), func(cur []byte) []byte { return append(append([]byte(nil), v...), cur...) })
		}
	}
	return tr
}

func hk(s string) string { return hex.EncodeToString([]byte(s)) }

func TestGenerateEngineGolden(t *testing.T) {
	root := os.Getenv("ELYSIANDB_GOLDEN_OUT")
	if root == "" {
		t.Skip("ELYSIANDB_GOLDEN_OUT is not set")
	}
	dir := filepath.Join(root, "engine")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	var lines goldenio.Lines
	for _, n := range []int{0, 1, 47, 48, 49, 2305, 20000} {
		for _, withVals := range []bool{false, true} {
			store := &memStore{pages: map[uint64][]byte{}}
			keys := make([]string, n)
			var vals [][]byte
			if withVals {
				vals = make([][]byte, n)
			}
			for i := range keys {
				keys[i] = fmt.Sprintf("k%06d", i)
				if withVals {
					vals[i] = fbytes(i%9, i)
				}
			}
			tr := FromSorted(store, keys, vals)
			pages := store.flush(t, tr)
			desc := map[string]any{"from_sorted": n, "keys": "k%06d", "values": nil}
			if withVals {
				desc["values"] = "i%9 bytes, byte j = ((j+i)*31+7)&0xff"
			}
			lines.Add(treeResult(t, store, fmt.Sprintf("from-sorted-%d-%v", n, withVals), desc, tr, pages))
		}
	}
	for _, size := range []int{300, 4000, 4097, 9000} {
		store := &memStore{pages: map[uint64][]byte{}}
		n := 40
		keys := make([]string, n)
		vals := make([][]byte, n)
		for i := range keys {
			keys[i] = fmt.Sprintf("k%06d", i)
			vals[i] = fbytes(size, i)
		}
		tr := FromSorted(store, keys, vals)
		pages := store.flush(t, tr)
		lines.Add(treeResult(t, store, fmt.Sprintf("from-sorted-heavy-%d", size), map[string]any{"from_sorted": n, "keys": "k%06d", "values": fmt.Sprintf("%d bytes, byte j = ((j+i)*31+7)&0xff", size)}, tr, pages))
	}
	scripts := map[string][]op{}
	var ops []op
	for i := 0; i < 200; i++ {
		ops = append(ops, op{Op: "put", Key: hk(fmt.Sprintf("s%04d", i)), Val: hex.EncodeToString(fbytes(8, i))})
	}
	scripts["splits-by-count"] = ops
	ops = nil
	for i := 0; i < 40; i++ {
		ops = append(ops, op{Op: "put", Key: hk(fmt.Sprintf("b%04d", (i*13)%40)), Val: hex.EncodeToString(fbytes(1000, i))})
	}
	scripts["splits-by-bytes"] = ops
	ops = nil
	for i := 0; i < 300; i++ {
		ops = append(ops, op{Op: "put", Key: hk(fmt.Sprintf("m%04d", i)), Val: "00"})
	}
	for i := 0; i < 300; i++ {
		if i%6 != 0 {
			ops = append(ops, op{Op: "delete", Key: hk(fmt.Sprintf("m%04d", i))})
		}
	}
	scripts["merges"] = ops
	ops = nil
	for i := 0; i < 100; i++ {
		ops = append(ops, op{Op: "put", Key: hk(fmt.Sprintf("r%04d", i)), Val: "01"})
	}
	for i := 1; i < 100; i++ {
		ops = append(ops, op{Op: "delete", Key: hk(fmt.Sprintf("r%04d", i))})
	}
	scripts["root-collapse"] = ops
	scripts["upsert-update"] = []op{
		{Op: "upsert", Key: hk("a"), Val: "01"}, {Op: "upsert", Key: hk("a"), Val: "02"}, {Op: "update", Key: hk("a"), Val: "03"},
		{Op: "update", Key: hk("missing"), Val: "04"}, {Op: "delete", Key: hk("missing")}, {Op: "put", Key: hk("b"), Val: ""}, {Op: "upsert", Key: hk("b"), Val: "05"},
	}
	scripts["keys-00-ff"] = []op{
		{Op: "put", Key: "00", Val: "aa"}, {Op: "put", Key: "ff", Val: "bb"}, {Op: "put", Key: "00ff", Val: "cc"}, {Op: "put", Key: "ff00", Val: "dd"},
		{Op: "put", Key: "", Val: "ee"}, {Op: "put", Key: "0000", Val: "11"}, {Op: "delete", Key: "ff"}, {Op: "put", Key: "ffffff", Val: "22"},
	}
	for _, name := range []string{"splits-by-count", "splits-by-bytes", "merges", "root-collapse", "upsert-update", "keys-00-ff"} {
		store := &memStore{pages: map[uint64][]byte{}}
		tr := runOps(New(store).WithToken(NewToken()), scripts[name])
		pages := store.flush(t, tr)
		lines.Add(treeResult(t, store, name, scripts[name], tr, pages))
	}
	store := &memStore{pages: map[uint64][]byte{}}
	tok := NewToken()
	base := runOps(New(store).WithToken(tok), scripts["splits-by-count"])
	first := store.flush(t, base)
	tok.Freeze()
	var cow []op
	for i := 0; i < 200; i += 7 {
		cow = append(cow, op{Op: "put", Key: hk(fmt.Sprintf("s%04d", i)), Val: "ff"})
	}
	cow = append(cow, op{Op: "delete", Key: hk("s0001")}, op{Op: "put", Key: hk("s9999"), Val: "ee"})
	next := runOps(base.WithToken(NewToken()), cow)
	second := store.flush(t, next)
	lines.Add(treeResult(t, store, "copy-on-write", map[string]any{"first": scripts["splits-by-count"], "second": cow}, next, append(first, second...)))
	for _, b := range []string{"0100", "0300", "01838004", "02010161000000000000000000000a01", "0101016101", "0180", "01010561", "", "02010161010203"} {
		raw, _ := hex.DecodeString(b)
		_, err := Decode(raw)
		row := map[string]any{"bytes": b}
		if err != nil {
			row["error"] = err.Error()
		} else {
			row["ok"] = true
		}
		lines.Add(row)
	}
	if err := lines.Write(dir, "pages.jsonl"); err != nil {
		t.Fatal(err)
	}
}
