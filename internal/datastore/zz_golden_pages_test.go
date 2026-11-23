package datastore

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"hash/crc32"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/internal/durable"
	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/pagetree"
)

func treeState(name, step string, tr pagetree.Tree, ops any) map[string]any {
	row := map[string]any{"tree": name, "step": step, "len": tr.Len(), "ops": ops}
	if r := tr.Root(); r != nil {
		row["root"] = map[string]any{"id": dec64(r.ID()), "size": r.Size()}
	} else {
		row["root"] = nil
	}
	return row
}

func TestGenerateGoldenPages(t *testing.T) {
	dir := goldenSet(t, "pages")
	work := filepath.Join(t.TempDir(), "pages")
	store, err := durable.OpenPageStore(nil, work, 1<<30, 16<<10)
	if err != nil {
		t.Fatal(err)
	}
	var steps []map[string]any
	flush := func(name, step string, tr pagetree.Tree, ops any) {
		if err := store.FlushTree(tr); err != nil {
			t.Fatal(err)
		}
		if err := store.Sync(); err != nil {
			t.Fatal(err)
		}
		steps = append(steps, treeState(name, step, tr, ops))
	}
	key := func(i int) string { return fmt.Sprintf("key-%08d", i) }

	tok := pagetree.NewToken()
	a := pagetree.New(store).WithToken(tok)
	for i := 0; i < 2000; i++ {
		a, _ = a.Put(key(i), formulaBytes(32, i))
	}
	tok.Freeze()
	flush("a", "put", a, map[string]any{"put": map[string]any{"from": 0, "to": 2000, "key": "key-%08d", "value": "32 bytes, byte j = ((j+i)*31+7)&0xff"}})

	tok = pagetree.NewToken()
	b := pagetree.New(store).WithToken(tok)
	for i := 0; i < 500; i++ {
		b, _ = b.Put(key(i), formulaBytes(32, i))
	}
	for i := 0; i < 500; i += 5 {
		b, _ = b.Delete(key(i))
		b, _ = b.Delete(key(i + 1))
	}
	tok.Freeze()
	flush("b", "put-delete", b, map[string]any{"put": map[string]any{"from": 0, "to": 500}, "delete": "keys i and i+1 for i = 0, 5, 10 \u2026 495"})

	var idxKeys []string
	for i := 0; i < 5000; i++ {
		idxKeys = append(idxKeys, fmt.Sprintf("f\x00title\x00%08d\x00%06d", i*7%5000, i))
	}
	sort.Strings(idxKeys)
	c1 := pagetree.FromSorted(store, idxKeys, nil)
	flush("c-index", "from-sorted", c1, map[string]any{"keys": "sorted fmt(\"f\\x00title\\x00%08d\\x00%06d\", i*7%5000, i) for i < 5000", "values": nil})

	var seqKeys []string
	var seqVals [][]byte
	for i := 0; i < 3000; i++ {
		var k [4]byte
		binary.BigEndian.PutUint32(k[:], uint32(i+1))
		seqKeys = append(seqKeys, string(k[:]))
		seqVals = append(seqVals, encodeSlot(nil, slot{seq: uint32(i + 1), id: key(i), lsn: uint64(i + 10), loc: journal.Location{Segment: uint32(1 + i/100), Offset: uint32(16 + i*40), Length: uint32(i % 97)}}))
	}
	c2 := pagetree.FromSorted(store, seqKeys, seqVals)
	flush("c-slots", "from-sorted", c2, map[string]any{"keys": "big-endian uint32 seq 1..3000", "values": "encodeSlot{seq, id key-%08d(i), lsn i+10, segment 1+i/100, offset 16+i*40, length i%97}"})

	d := pagetree.New(store)
	flush("d", "empty", d, nil)

	tok = pagetree.NewToken()
	e := a.WithToken(tok)
	for i := 0; i < 100; i++ {
		e, _ = e.Put(key(i*17), formulaBytes(48, i+1000))
	}
	tok.Freeze()
	flush("e", "update-after-flush", e, map[string]any{"put": "key-%08d(i*17) for i < 100, 48 bytes, byte j = ((j+i+1000)*31+7)&0xff"})

	if err := store.Close(); err != nil {
		t.Fatal(err)
	}
	copyTree(t, work, filepath.Join(dir, "pgs"))
	goldenJSON(t, filepath.Join(dir, "trees.json"), map[string]any{"store": map[string]any{"limit": dec64(1 << 30), "segment_size": 16 << 10}, "steps": steps})
}

func pageDump(p *pagetree.Page) map[string]any {
	enc, err := pagetree.Encode(nil, p)
	row := map[string]any{"leaf": p.Leaf(), "len": p.Len(), "total": dec64(p.Total())}
	if err == nil {
		row["reencoded"] = hexOf(enc)
	}
	var kids []map[string]any
	for _, k := range p.Kids() {
		kids = append(kids, map[string]any{"id": dec64(k.ID()), "size": k.Size()})
	}
	if kids != nil {
		row["kids"] = kids
	}
	return row
}

func TestGenerateGoldenPageDecode(t *testing.T) {
	path := goldenFile(t, "pages/decode.json")
	uv := func(v uint64) []byte { return binary.AppendUvarint(nil, v) }
	cat := func(parts ...[]byte) []byte {
		var out []byte
		for _, p := range parts {
			out = append(out, p...)
		}
		return out
	}
	child := func(id uint64, size, total uint64) []byte {
		return cat(binary.LittleEndian.AppendUint64(nil, id), uv(size), uv(total))
	}
	leaf2 := cat([]byte{1}, uv(2), uv(1), []byte("a"), uv(2), []byte("xy"), uv(1), []byte("b"), uv(0))
	inner2 := cat([]byte{2}, uv(2), uv(1), []byte("a"), child(1<<32|16, 40, 3), uv(1), []byte("m"), child(1<<32|64, 50, 4))
	inputs := []struct {
		name string
		b    []byte
	}{
		{"empty-leaf", []byte{0x01, 0x00}},
		{"empty-inner", []byte{0x02, 0x00}},
		{"leaf-two", leaf2},
		{"inner-two", inner2},
		{"kind-3", []byte{0x03, 0x00}},
		{"kind-0", []byte{0x00, 0x00}},
		{"one-byte", []byte{0x01}},
		{"nothing", nil},
		{"n-65536", cat([]byte{1}, uv(1<<16))},
		{"n-65537", cat([]byte{1}, uv(1<<16+1))},
		{"child-id-0", cat([]byte{2}, uv(1), uv(1), []byte("a"), child(0, 10, 1))},
		{"trailing-byte", append(append([]byte(nil), leaf2...), 0x00)},
		{"truncated-varint", []byte{0x01, 0x80}},
		{"length-beyond-end", cat([]byte{1}, uv(1), uv(5), []byte("ab"))},
		{"value-beyond-end", cat([]byte{1}, uv(1), uv(1), []byte("a"), uv(9), []byte("x"))},
		{"inner-short-id", cat([]byte{2}, uv(1), uv(1), []byte("a"), []byte{1, 2, 3})},
		{"inner-missing-total", cat([]byte{2}, uv(1), uv(1), []byte("a"), binary.LittleEndian.AppendUint64(nil, 5), uv(3))},
		{"count-too-high", cat([]byte{1}, uv(3), uv(1), []byte("a"), uv(0))},
		{"key-with-nul-ff", cat([]byte{1}, uv(1), uv(3), []byte{0x00, 0xff, 0x00}, uv(1), []byte{0xfe})},
	}
	var rows []map[string]any
	for _, in := range inputs {
		row := map[string]any{"name": in.name, "hex": hexOf(in.b)}
		p, err := pagetree.Decode(in.b)
		switch {
		case err == nil:
			row["page"] = pageDump(p)
		case errors.Is(err, pagetree.ErrCorruptPage):
			row["error"] = err.Error()
		default:
			t.Fatalf("%s: unexpected error kind", in.name)
		}
		rows = append(rows, row)
	}
	goldenJSON(t, path, rows)
}

func kvRecordsJSON(recs []durable.KVRecord) []map[string]any {
	out := []map[string]any{}
	for _, r := range recs {
		out = append(out, map[string]any{"key_hex": hexOf([]byte(r.Key)), "lsn": dec64(r.LSN), "expires_at": decI64(r.ExpiresAt), "segment": r.Segment, "offset": r.Offset, "length": r.Length})
	}
	return out
}

func TestGenerateGoldenKVFile(t *testing.T) {
	dir := goldenSet(t, "kvfile")
	var thousand []durable.KVRecord
	for i := 0; i < 1000; i++ {
		thousand = append(thousand, durable.KVRecord{Key: fmt.Sprintf("key-%08d", i), LSN: uint64(i + 1), ExpiresAt: int64(i%3) * 4102444800, Segment: uint32(1 + i/250), Offset: uint32(16 + i*24), Length: uint32(i % 50)})
	}
	sets := []struct {
		name string
		recs []durable.KVRecord
	}{
		{"none", nil},
		{"a", []durable.KVRecord{{Key: "a", LSN: 1, Segment: 1, Offset: 16, Length: 1}}},
		{"long", []durable.KVRecord{{Key: "\u00e9:long", LSN: 1 << 40, ExpiresAt: -3, Segment: 7, Offset: 1 << 31, Length: 1 << 20}}},
		{"odd-keys", []durable.KVRecord{{Key: "\xff\xfe", LSN: 2, Segment: 1, Offset: 30, Length: 0}, {Key: "a\u2028b", LSN: 3, ExpiresAt: 1800000000, Segment: 1, Offset: 40, Length: 5}}},
		{"thousand", thousand},
	}
	var cases []map[string]any
	for _, s := range sets {
		p := filepath.Join(dir, s.name+".ckpt")
		if err := durable.WriteKVFile(nil, p, s.recs); err != nil {
			t.Fatal(err)
		}
		got, err := durable.ReadKVFile(nil, p)
		if err != nil {
			t.Fatal(err)
		}
		cases = append(cases, map[string]any{"file": s.name + ".ckpt", "written": kvRecordsJSON(s.recs), "read": kvRecordsJSON(got)})
	}
	base, err := os.ReadFile(filepath.Join(dir, "odd-keys.ckpt"))
	if err != nil {
		t.Fatal(err)
	}
	reseal := func(body []byte) []byte {
		out := append([]byte(nil), body...)
		return binary.LittleEndian.AppendUint32(out, crc32.Checksum(out, castagnoli))
	}
	body := base[:len(base)-4]
	overflow := append([]byte(nil), body[:8]...)
	overflow = binary.AppendUvarint(overflow, 1)
	overflow = binary.AppendUvarint(overflow, 1<<40)
	overflow = append(overflow, 'k')
	corrupt := []struct {
		name string
		b    []byte
	}{
		{"flip-12", func() []byte { c := append([]byte(nil), base...); c[12] ^= 0x01; return c }()},
		{"truncated", base[:len(base)-3]},
		{"wrong-magic", func() []byte { c := append([]byte(nil), base...); c[0] = 'X'; return reseal(c[:len(c)-4]) }()},
		{"extra-byte", reseal(append(append([]byte(nil), body...), 0x00))},
		{"key-length-overflow", reseal(overflow)},
		{"short", []byte("ELYKVCK")},
		{"empty", []byte{}},
	}
	for _, c := range corrupt {
		p := filepath.Join(dir, "corrupt-"+c.name+".ckpt")
		if err := os.WriteFile(p, c.b, 0o644); err != nil {
			t.Fatal(err)
		}
		row := map[string]any{"file": "corrupt-" + c.name + ".ckpt"}
		got, err := durable.ReadKVFile(nil, p)
		switch {
		case err == nil:
			row["read"] = kvRecordsJSON(got)
		case errors.Is(err, durable.ErrCorruptFile):
			row["error"] = err.Error()
		default:
			t.Fatalf("%s: unexpected error kind", c.name)
		}
		cases = append(cases, row)
	}
	goldenJSON(t, filepath.Join(dir, "cases.json"), cases)
}

func TestGenerateGoldenManifest(t *testing.T) {
	dir := goldenSet(t, "manifest")
	full := manifest{
		Format: manifestFormatPages, Checkpoint: "checkpoint-0000000000000192.ckpt", KV: "kv-0000000000000200.ckpt", LSN: 200, Segment: 3, Offset: 4096,
		CreatedAt: "<created_at>", Version: "0.3.0", Types: []string{"book", "c<&>", "a\u2028b\u2029"},
		Entities: []manifestEntity{{
			Name: "book", Gen: 1 << 40, NextSeq: 301, Version: 77,
			Refs:  []RefPath{{Path: "author", Target: "person"}, {Path: "tags.*", Target: "tag"}},
			Slots: pageRoot{ID: 1<<32 | 16, Size: 4000}, IDs: pageRoot{ID: 2<<32 | 32, Size: 3000},
			Indexes: []manifestIndex{{Name: "f:title", Root: pageRoot{ID: 3<<32 | 16, Size: 100}}, {Name: "s:year", Root: pageRoot{ID: 0, Size: 0}, Dormant: true}},
			Derived: []manifestIndex{{Name: "c:status,year", Root: pageRoot{ID: 4<<32 | 16, Size: 80}}},
			Origins: map[string]string{"f:title": "hook", "s:year": "query"},
		}},
		Live:     map[string]int64{"1": 100, "2": 2000, "10": 5},
		Previous: 150,
	}
	var controls strings.Builder
	for c := 1; c < 0x20; c++ {
		controls.WriteByte(byte(c))
	}
	controls.WriteByte(0x7f)
	encodeCases := []struct {
		name string
		m    manifest
	}{
		{"full", full},
		{"empty", manifest{}},
		{"format-1", manifest{Format: manifestFormat, Checkpoint: "checkpoint-0000000000000001.ckpt", LSN: 1, CreatedAt: "<created_at>"}},
		{"escapes", manifest{Format: manifestFormatPages, CreatedAt: "<created_at>", Types: []string{"<script>&", "\u2028\u2029", controls.String(), "\xff\xfe", "\"q\\"}}},
		{"live-order", manifest{Format: manifestFormatPages, CreatedAt: "<created_at>", Live: map[string]int64{"10": 1, "2": 2, "1": 3, "100": 4, "9": 5}}},
	}
	if err := os.MkdirAll(filepath.Join(dir, "encode"), 0o755); err != nil {
		t.Fatal(err)
	}
	for _, c := range encodeCases {
		data, err := json.MarshalIndent(c.m, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "encode", c.name+".json"), append(data, '\n'), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	parseInputs := []struct {
		name string
		in   string
	}{
		{"format-2", `{"format":2,"lsn":5,"segment":1,"offset":16,"created_at":"x"}`},
		{"format-1", `{"format":1,"checkpoint":"c.ckpt","lsn":5}`},
		{"upper-case-keys", `{"FORMAT":2,"LSN":7,"Segment":2,"CREATED_AT":"y"}`},
		{"duplicate-keys", `{"format":2,"lsn":1,"lsn":9}`},
		{"null", `null`},
		{"null-fields", `{"format":2,"types":null,"entities":null,"live":null}`},
		{"format-1.0", `{"format":1.0}`},
		{"format-1e0", `{"format":1e0}`},
		{"lsn-1e2", `{"format":2,"lsn":1e2}`},
		{"lsn-negative", `{"format":2,"lsn":-1}`},
		{"trailing-data", `{"format":2} x`},
		{"truncated", `{"format":2,"lsn":`},
		{"format-0", `{"format":0}`},
		{"format-3", `{"format":3}`},
		{"missing-format", `{"lsn":3}`},
		{"string-format", `{"format":"2"}`},
		{"live-bad-value", `{"format":2,"live":{"1":"x"}}`},
		{"live-9x", `{"format":2,"live":{"9x":1}}`},
		{"unknown-field", `{"format":2,"extra":[1,2]}`},
		{"empty", ``},
	}
	var parsed []map[string]any
	for _, p := range parseInputs {
		row := map[string]any{"name": p.name, "hex": hexOf([]byte(p.in))}
		m, err := parseManifest([]byte(p.in))
		if err != nil {
			row["rejected"] = manifestRejection(err)
		} else {
			again, merr := json.Marshal(m)
			if merr != nil {
				t.Fatal(merr)
			}
			row["accepted"] = string(again)
		}
		parsed = append(parsed, row)
	}
	goldenJSON(t, filepath.Join(dir, "parse.json"), parsed)
}

func manifestRejection(err error) map[string]any {
	var se *json.SyntaxError
	var te *json.UnmarshalTypeError
	prefix := "cannot read " + ManifestFile + ": "
	msg := err.Error()
	switch {
	case errors.As(err, &se):
		return map[string]any{"literal": prefix, "wrapped": "json_syntax", "offset": se.Offset}
	case errors.As(err, &te):
		return map[string]any{"literal": prefix, "wrapped": "json_type", "field": te.Field, "offset": te.Offset}
	case strings.HasPrefix(msg, prefix):
		return map[string]any{"literal": prefix, "wrapped": "json_other"}
	}
	return map[string]any{"message": msg}
}

func checkpointDump(ck *checkpointData) map[string]any {
	ents := []map[string]any{}
	for _, e := range ck.entities {
		refs := []map[string]any{}
		for _, r := range e.refs {
			refs = append(refs, map[string]any{"path_hex": hexOf([]byte(r.Path)), "target_hex": hexOf([]byte(r.Target))})
		}
		slots := []map[string]any{}
		for _, s := range e.slots {
			slots = append(slots, map[string]any{"seq": s.seq, "id_hex": hexOf([]byte(s.id)), "lsn": dec64(s.lsn), "segment": s.loc.Segment, "offset": s.loc.Offset, "length": s.loc.Length})
		}
		idx := []map[string]any{}
		for _, ix := range e.indexes {
			keys := []string{}
			for _, k := range ix.keys {
				keys = append(keys, hexOf([]byte(k)))
			}
			idx = append(idx, map[string]any{"name": ix.spec.Name(), "keys_hex": keys})
		}
		ents = append(ents, map[string]any{"name_hex": hexOf([]byte(e.name)), "gen": dec64(e.gen), "next_seq": e.nextSeq, "version": dec64(e.version), "refs": refs, "slots": slots, "indexes": idx})
	}
	kv := []map[string]any{}
	for _, k := range ck.kv {
		kv = append(kv, map[string]any{"key_hex": hexOf([]byte(k.key)), "lsn": dec64(k.entry.lsn), "expires_at": decI64(k.entry.expiresAt), "segment": k.entry.loc.Segment, "offset": k.entry.loc.Offset, "length": k.entry.loc.Length})
	}
	var extras any
	if ck.extras != nil {
		names := make([]string, 0, len(ck.extras))
		for n := range ck.extras {
			names = append(names, n)
		}
		sort.Strings(names)
		list := []map[string]any{}
		for _, n := range names {
			x := ck.extras[n]
			dormant := []string{}
			for _, d := range x.dormant {
				dormant = append(dormant, d.Name())
			}
			list = append(list, map[string]any{"entity_hex": hexOf([]byte(n)), "dormant": dormant, "search": x.search})
		}
		extras = list
	}
	types := []string{}
	for _, ty := range ck.types {
		types = append(types, hexOf([]byte(ty)))
	}
	return map[string]any{"last_lsn": dec64(ck.lastLSN), "position": map[string]any{"segment": ck.pos.Segment, "offset": ck.pos.Offset}, "types_hex": types, "entities": ents, "kv": kv, "extras": extras}
}

func TestGenerateGoldenFormat1(t *testing.T) {
	dir := goldenSet(t, "format1")
	src := filepath.Join("..", "..", "test", "compat", "testdata", "pr-format1", "format1", "checkpoint-0000000000000192.ckpt")
	base, err := os.ReadFile(src)
	if err != nil {
		t.Fatal(err)
	}
	ck, err := readCheckpoint(src)
	if err != nil {
		t.Fatal(err)
	}
	goldenJSON(t, filepath.Join(dir, "checkpoint.json"), map[string]any{"source": "tests/compat/testdata/pr-format1/format1/checkpoint-0000000000000192.ckpt", "dump": checkpointDump(ck)})
	reseal := func(body []byte) []byte {
		out := append([]byte(nil), body...)
		return binary.LittleEndian.AppendUint32(out, crc32.Checksum(out, castagnoli))
	}
	body := base[:len(base)-4]
	version2 := append([]byte(nil), body...)
	version2[len(checkpointMagic)] = 2
	cases := []struct {
		name string
		b    []byte
	}{
		{"flip", func() []byte { c := append([]byte(nil), base...); c[len(c)/2] ^= 0x10; return c }()},
		{"truncated", base[:len(base)-9]},
		{"version-2", reseal(version2)},
		{"trailing-bytes", reseal(append(append([]byte(nil), body...), 0x07, 0x07))},
		{"cut-body-resealed", reseal(body[:len(body)/2])},
		{"short", []byte("ELYCKPT")},
		{"wrong-magic", reseal(append([]byte("ELYCKPT2"), body[8:]...))},
	}
	if dormant, ok := withUnparseableDormant(body); ok {
		cases = append(cases, struct {
			name string
			b    []byte
		}{"dormant-name-unparseable", reseal(dormant)})
	}
	var rows []map[string]any
	for _, c := range cases {
		p := filepath.Join(dir, "cases", c.name+".ckpt")
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, c.b, 0o644); err != nil {
			t.Fatal(err)
		}
		row := map[string]any{"file": "cases/" + c.name + ".ckpt"}
		got, err := readCheckpoint(p)
		if err != nil {
			row["error"] = err.Error()
		} else {
			row["dump"] = checkpointDump(got)
		}
		rows = append(rows, row)
	}
	goldenJSON(t, filepath.Join(dir, "cases.json"), rows)
}

func withUnparseableDormant(body []byte) ([]byte, bool) {
	r := &ckReader{data: body, pos: len(checkpointMagic)}
	r.uvarint()
	r.uvarint()
	r.uvarint()
	r.uvarint()
	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		r.str()
	}
	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		r.str()
		r.uvarint()
		r.uvarint()
		r.uvarint()
		for m := r.uvarint(); m > 0 && r.err == nil; m-- {
			r.str()
			r.str()
		}
		for m := r.uvarint(); m > 0 && r.err == nil; m-- {
			r.uvarint()
			r.str()
			r.uvarint()
			r.loc()
		}
		for m := r.uvarint(); m > 0 && r.err == nil; m-- {
			r.str()
			for k := r.uvarint(); k > 0 && r.err == nil; k-- {
				r.str()
			}
		}
	}
	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		r.str()
		r.uvarint()
		r.varint()
		r.loc()
	}
	if r.err != nil {
		return nil, false
	}
	out := append([]byte(nil), body[:r.pos]...)
	out = binary.AppendUvarint(out, ckExtrasSection)
	out = binary.AppendUvarint(out, 1)
	out = binary.AppendUvarint(out, 4)
	out = append(out, "book"...)
	out = binary.AppendUvarint(out, 2)
	out = binary.AppendUvarint(out, 7)
	out = append(out, "f:title"...)
	out = binary.AppendUvarint(out, 2)
	out = append(out, "xx"...)
	out = binary.AppendUvarint(out, 1)
	return out, true
}
