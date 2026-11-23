package datastore

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/durable"
	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/legacy"
	"github.com/taymour/elysiandb/internal/pagetree"
)

const fixedCreatedAt = "2000-01-01T00:00:00Z"

var createdAtField = regexp.MustCompile(`"created_at": "[^"]*"`)

func folderOptions(dir string) Options {
	return Options{Folder: dir, Shards: 8, Durability: journal.ModeSync, SegmentSize: 4 << 10, PageSegmentSize: 16 << 10, PageCacheBytes: 8 << 10, CacheBytes: 4 << 10, LegacyRecovery: true}
}

func maskManifests(t *testing.T, dir string) {
	t.Helper()
	for _, name := range []string{durable.ManifestFile, durable.PreviousManifest} {
		p := filepath.Join(dir, name)
		data, err := os.ReadFile(p)
		if errors.Is(err, fs.ErrNotExist) {
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		masked := createdAtField.ReplaceAll(data, []byte(`"created_at": "`+fixedCreatedAt+`"`))
		if err := os.WriteFile(p, masked, 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

func mustUpdate(t *testing.T, e *Engine, fn func(tx *Tx) error) {
	t.Helper()
	if err := e.Update(fn); err != nil {
		t.Fatal(err)
	}
}

func docID(i int) string {
	if i%50 == 7 {
		return fmt.Sprintf("id-\xff%04d", i)
	}
	return fmt.Sprintf("id-%04d", i)
}

func workloadDoc(i int) map[string]any {
	doc := map[string]any{
		"title":  fmt.Sprintf("T%03d", i%37),
		"n":      float64(i),
		"status": []string{"open", "closed", "draft"}[i%3],
		"date":   fmt.Sprintf("2026-01-%02dT10:00:00Z", 1+i%28),
		"tags":   []any{"x", "y", "z"}[:i%4],
	}
	if i%10 == 3 {
		doc["author"] = map[string]any{"@entity": "a", "id": docID(i % 30)}
	}
	if i%25 == 4 {
		doc["notes"] = []any{map[string]any{"@entity": "b", "id": docID(i % 20)}, "plain", 1.5}
	}
	return doc
}

func runWorkload(t *testing.T, dir string) {
	t.Helper()
	e, err := Open(folderOptions(dir))
	if err != nil {
		t.Fatal(err)
	}
	types := []string{"a", "b", "c<&>"}
	mustUpdate(t, e, func(tx *Tx) error {
		for _, ty := range append(types, "d") {
			tx.EnsureType(ty)
		}
		return nil
	})
	for start := 0; start < 300; start += 25 {
		mustUpdate(t, e, func(tx *Tx) error {
			for i := start; i < start+25; i++ {
				if err := tx.Put(types[i%3], docID(i), workloadDoc(i)); err != nil {
					return err
				}
			}
			return nil
		})
	}
	mustUpdate(t, e, func(tx *Tx) error { return tx.Put("d", "only", map[string]any{"x": 1.0}) })
	specs := []struct {
		entity string
		spec   IndexSpec
		origin IndexOrigin
	}{
		{"a", IndexSpec{Kind: IndexFilter, Field: "title"}, OriginQuery},
		{"a", IndexSpec{Kind: IndexSort, Field: "n"}, OriginQuery},
		{"a", IndexSpec{Kind: IndexRefs, Field: "author"}, OriginQuery},
		{"a", IndexSpec{Kind: IndexSearch, Field: "title"}, OriginQuery},
		{"a", IndexSpec{Kind: IndexBitmap, Field: "status"}, OriginAdvisor},
		{"a", IndexSpec{Kind: IndexComposite, Field: "status,n"}, OriginAdvisor},
		{"b", IndexSpec{Kind: IndexFilter, Field: "title"}, OriginHook},
		{"b", IndexSpec{Kind: IndexSort, Field: "date"}, OriginQuery},
		{"c<&>", IndexSpec{Kind: IndexFilter, Field: "tags"}, OriginQuery},
	}
	for _, s := range specs {
		e.EnsureIndexWith(s.entity, s.spec, s.origin)
	}
	dormant := IndexSpec{Kind: IndexSort, Field: "n"}
	for _, s := range specs {
		if !(s.entity == "a" && s.spec == dormant) {
			e.PinIndex(s.entity, s.spec)
		}
	}
	e.DropIdleIndexes(0)
	time.Sleep(5 * time.Millisecond)
	e.DropIdleIndexes(0)
	for _, s := range specs {
		if !(s.entity == "a" && s.spec == dormant) {
			e.UnpinIndex(s.entity, s.spec)
		}
	}
	mustUpdate(t, e, func(tx *Tx) error {
		for i := 0; i < 300; i += 11 {
			tx.Delete(types[i%3], docID(i))
		}
		return nil
	})
	if err := e.KVPut("plain", []byte("value"), 0); err != nil {
		t.Fatal(err)
	}
	if err := e.KVPut("ttl-2100", []byte("expires in 2100"), 4102444800); err != nil {
		t.Fatal(err)
	}
	if err := e.KVPut("binary\xff", []byte{0, 1, 2, 0xff}, 0); err != nil {
		t.Fatal(err)
	}
	if err := e.KVPut("gone", []byte("x"), 0); err != nil {
		t.Fatal(err)
	}
	e.KVDelete("gone")
	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}
	mustUpdate(t, e, func(tx *Tx) error { tx.ResetEntity("b"); return nil })
	mustUpdate(t, e, func(tx *Tx) error {
		for i := 0; i < 10; i++ {
			if err := tx.Put("b", fmt.Sprintf("again-%02d", i), workloadDoc(i+1000)); err != nil {
				return err
			}
		}
		return nil
	})
	mustUpdate(t, e, func(tx *Tx) error { tx.DropType("d"); return nil })
	for i := 0; i < 40; i++ {
		mustUpdate(t, e, func(tx *Tx) error { return tx.Put("a", docID(i), workloadDoc(i+2000)) })
	}
	if _, err := e.Compact(2); err != nil {
		t.Fatal(err)
	}
	if _, err := e.CompactPages(); err != nil {
		t.Fatal(err)
	}
	if err := e.Checkpoint(); err != nil {
		t.Fatal(err)
	}
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
}

func folderDump(t *testing.T, e *Engine) map[string]any {
	t.Helper()
	types := e.Types()
	ents := []map[string]any{}
	for _, name := range types {
		s := e.Snapshot(name)
		if s == nil {
			ents = append(ents, map[string]any{"name_hex": hexOf([]byte(name)), "snapshot": nil})
			continue
		}
		docs := []map[string]any{}
		s.Ascend(0, func(d Doc) bool {
			raw, err := d.Bytes()
			row := map[string]any{"id_hex": hexOf([]byte(d.ID())), "seq": d.Seq(), "lsn": dec64(d.LSN())}
			if err != nil {
				row["read_error"] = true
			} else {
				row["raw_b64"] = base64.StdEncoding.EncodeToString(raw)
			}
			docs = append(docs, row)
			return true
		})
		refs := []map[string]any{}
		for _, r := range s.Refs() {
			refs = append(refs, map[string]any{"path_hex": hexOf([]byte(r.Path)), "target_hex": hexOf([]byte(r.Target))})
		}
		idx := []map[string]any{}
		for _, iname := range e.IndexNames(name) {
			spec, ok := ParseIndexName(iname)
			row := map[string]any{"name": iname}
			if !ok {
				row["parsed"] = false
				idx = append(idx, row)
				continue
			}
			row["dormant"] = s.IndexDormant(spec)
			if v, ok := s.Index(spec); ok {
				keys := [][]any{}
				v.ScanRange(nil, nil, func(key string, seq uint32) bool {
					keys = append(keys, []any{hexOf([]byte(key)), seq})
					return true
				})
				row["entries"] = keys
				row["len"] = v.Len()
			}
			idx = append(idx, row)
		}
		ents = append(ents, map[string]any{
			"name_hex": hexOf([]byte(name)), "gen": dec64(s.Generation()), "next_seq": s.NextSeq(), "version": dec64(s.Version()), "len": s.Len(),
			"docs": docs, "refs": refs, "indexes": idx,
		})
	}
	states := []map[string]any{}
	for _, st := range e.IndexStates() {
		states = append(states, map[string]any{"entity_hex": hexOf([]byte(st.Entity)), "index": st.Index, "kind": st.Kind, "origin": st.Origin, "state": st.State, "pinned": st.Pinned, "entries": st.Entries})
	}
	sort.Slice(states, func(i, j int) bool {
		a, b := states[i], states[j]
		if a["entity_hex"].(string) != b["entity_hex"].(string) {
			return a["entity_hex"].(string) < b["entity_hex"].(string)
		}
		return a["index"].(string) < b["index"].(string)
	})
	keys := append([]string(nil), e.KVKeys()...)
	sort.Strings(keys)
	kv := []map[string]any{}
	for _, k := range keys {
		v, exp, ok := e.KVGet(k)
		row := map[string]any{"key_hex": hexOf([]byte(k)), "found": ok, "value_hex": hexOf(v), "expires_at": decI64(exp)}
		sh := e.kv.shard(k)
		sh.mu.RLock()
		if ent := sh.m[k]; ent != nil {
			row["lsn"] = dec64(ent.lsn)
		}
		sh.mu.RUnlock()
		kv = append(kv, row)
	}
	typesHex := []string{}
	for _, ty := range types {
		typesHex = append(typesHex, hexOf([]byte(ty)))
	}
	return map[string]any{"types_hex": typesHex, "entities": ents, "index_states": states, "kv": kv, "replayed_records": e.ReplayedRecords()}
}

func dumpCopy(t *testing.T, before string) map[string]any {
	t.Helper()
	work := filepath.Join(t.TempDir(), "data")
	copyTree(t, before, work)
	e, err := Open(folderOptions(work))
	if err != nil {
		t.Fatal(err)
	}
	dump := folderDump(t, e)
	if m := e.Migration(); m != nil {
		dump["migration"] = map[string]any{"entities": m.Entities, "documents": m.Documents, "kv_keys": m.KVKeys, "replayed": m.Replayed}
	}
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
	return dump
}

func flipMiddle(t *testing.T, path string) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	data[len(data)/2] ^= 0x20
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatal(err)
	}
}

func lastSegment(t *testing.T, logDir string) string {
	t.Helper()
	entries, err := os.ReadDir(logDir)
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, e := range entries {
		if strings.HasSuffix(e.Name(), ".seg") {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)
	if len(names) == 0 {
		t.Fatal("no log segment")
	}
	return filepath.Join(logDir, names[len(names)-1])
}

func TestGenerateGoldenFolders(t *testing.T) {
	dir := goldenSet(t, "folders")
	root := t.TempDir()
	clean := filepath.Join(root, "clean")
	runWorkload(t, clean)
	maskManifests(t, clean)
	save := func(name, src string) {
		before := filepath.Join(dir, name, "before")
		copyTree(t, src, before)
		goldenJSON(t, filepath.Join(dir, name, "expected.json"), dumpCopy(t, before))
	}
	save("clean", clean)

	crash := filepath.Join(root, "crash")
	copyTree(t, clean, crash)
	e, err := Open(folderOptions(crash))
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 30; i++ {
		mustUpdate(t, e, func(tx *Tx) error { return tx.Put("c<&>", fmt.Sprintf("crash-%02d", i), workloadDoc(i+3000)) })
	}
	mustUpdate(t, e, func(tx *Tx) error { tx.Delete("a", docID(1)); return nil })
	if err := e.KVPut("after-crash", []byte("kept"), 0); err != nil {
		t.Fatal(err)
	}
	e.Abandon()
	maskManifests(t, crash)
	save("crash", crash)

	damaged := filepath.Join(root, "damaged")
	copyTree(t, crash, damaged)
	flipMiddle(t, lastSegment(t, filepath.Join(damaged, LogFolder)))
	e, err = Open(folderOptions(damaged))
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
	maskManifests(t, damaged)
	save("damaged", damaged)

	fixtures := filepath.Join("..", "..", "test", "compat", "testdata")
	for _, f := range []struct{ name, src string }{
		{"format1-upgraded", filepath.Join(fixtures, "pr-format1", "format1")},
		{"migrated", filepath.Join(fixtures, "v0.2.4", "full")},
	} {
		var work string
		for attempt := 0; ; attempt++ {
			work = filepath.Join(root, fmt.Sprintf("%s-%d", f.name, attempt))
			copyTree(t, f.src, work)
			e, err := Open(folderOptions(work))
			if err != nil {
				t.Fatal(err)
			}
			if err := e.Close(); err != nil {
				t.Fatal(err)
			}
			if kvInKeyOrder(t, work) {
				break
			}
			if attempt == 500 {
				t.Fatalf("%s: no run wrote the KV records in key order", f.name)
			}
		}
		maskManifests(t, work)
		save(f.name, work)
	}
}

func kvInKeyOrder(t *testing.T, dir string) bool {
	t.Helper()
	dump := dumpCopy(t, dir)
	var last uint64
	for _, row := range dump["kv"].([]map[string]any) {
		lsnText, ok := row["lsn"].(string)
		if !ok {
			continue
		}
		var lsn uint64
		if _, err := fmt.Sscan(lsnText, &lsn); err != nil {
			t.Fatal(err)
		}
		if lsn < last {
			return false
		}
		last = lsn
	}
	return true
}

type legacyCase struct {
	name   string
	files  map[string]string
	replay bool
}

func legacyCases() []legacyCase {
	b64 := func(s string) string { return base64.StdEncoding.EncodeToString([]byte(s)) }
	return []legacyCase{
		{"empty-folder", map[string]string{}, true},
		{"base64-with-newline", map[string]string{legacy.DataFile: `{"k":"` + b64("hello")[:4] + `\n` + b64("hello")[4:] + `"}`}, true},
		{"null-value", map[string]string{legacy.DataFile: `{"k":null,"j":"` + b64("v") + `"}`}, true},
		{"empty-files", map[string]string{legacy.DataFile: "", legacy.JsonDataFile: "  \n", legacy.ExpirationFile: ""}, true},
		{"invalid-json", map[string]string{legacy.DataFile: `{"k":`}, true},
		{"invalid-base64", map[string]string{legacy.DataFile: `{"k":"%%%"}`}, true},
		{"top-level-array", map[string]string{legacy.DataFile: `["a"]`}, true},
		{"expiration-keys", map[string]string{
			legacy.DataFile:       `{"a":"` + b64("1") + `","b":"` + b64("2") + `","c":"` + b64("3") + `","d":"` + b64("4") + `","e":"` + b64("5") + `"}`,
			legacy.ExpirationFile: `{"+12":["a"],"-5":["b"],"abc":["c"],"1799999999":["d"],"1800000000":["e"],"1800000001":["missing"]}`,
		}, true},
		{"recovery-logs", map[string]string{
			legacy.DataFile:          `{"keep":"` + b64("k") + `"}`,
			legacy.JsonDataFile:      `{"api:entity:book:id:1":{"title":"one"}}`,
			legacy.JsonRecoveryFile:  "{\"OP\":\"put\",\"KEY\":\"api:entity:book:id:2\",\"VALUE\":{\"title\":\"two\"}}\nnot json\n{\"op\":\"del\",\"key\":\"api:entity:book:id:1\"}\n{\"op\":\"noop\",\"key\":\"x\"}\n{\"op\":\"put\",\"key\":\"api:entity:book:id:3\",\"value\":{\"title\":\"three\"}}",
			legacy.StoreRecoveryFile: "{\"op\":\"put\",\"key\":\"t1\",\"value\":\"" + b64("v1") + "\",\"ttl\":1900000000}\n{\"op\":\"put\",\"key\":\"t2\",\"value\":\"" + b64("v2") + "\",\"ttl\":1700000000}\n{\"op\":\"put\",\"key\":\"t3\",\"value\":\"" + b64("v3") + "\",\"ttl\":1.5}\n{\"op\":\"del\",\"key\":\"keep\"}\n\n   \n{\"op\":\"put\",\"key\":\"t4\",\"value\":\"" + b64("v4") + "\"}",
		}, true},
		{"recovery-logs-not-replayed", map[string]string{
			legacy.JsonRecoveryFile:  "{\"op\":\"put\",\"key\":\"api:entity:book:id:2\",\"value\":{\"title\":\"two\"}}\n",
			legacy.StoreRecoveryFile: "{\"op\":\"put\",\"key\":\"t1\",\"value\":\"" + b64("v1") + "\"}\n",
		}, false},
		{"failed-files", map[string]string{
			legacy.JsonRecoveryFile + legacy.FailedSuffix:  "{\"op\":\"put\",\"key\":\"api:entity:book:id:9\",\"value\":{\"title\":\"failed\"}}\n",
			legacy.StoreRecoveryFile + legacy.FailedSuffix: "{\"op\":\"put\",\"key\":\"f\",\"value\":\"" + b64("f") + "\"}\n",
			legacy.DataFile: `{"x":"` + b64("x") + `"}`,
		}, true},
		{"types-and-ids", map[string]string{
			legacy.DataFile: `{"api:entity:types:list":"` + b64("book\n\nauthor\nbook\n") + `","api:entity:book:internal:index:id":"` + b64("2\n1\n2\nghost\n\n") + `"}`,
			legacy.JsonDataFile: `{"api:entity:book:id:1":{"t":1},"api:entity:book:id:2":{"t":2},"api:entity:book:id:3":{"t":3},"api:entity:zeta:id:z":{"t":"z"},"api:entity:alpha:id:a":{"t":"a"},` +
				`"api:entity::id:x":{"t":"bad"},"api:entity:book:id:":{"t":"bad"},"other:key":{"t":"other"}}`,
		}, true},
	}
}

func legacyDataJSON(d *legacy.Data) map[string]any {
	kvKeys := make([]string, 0, len(d.KV))
	for k := range d.KV {
		kvKeys = append(kvKeys, k)
	}
	sort.Strings(kvKeys)
	kv := []map[string]any{}
	for _, k := range kvKeys {
		kv = append(kv, map[string]any{"key_hex": hexOf([]byte(k)), "value_hex": hexOf(d.KV[k])})
	}
	expKeys := make([]string, 0, len(d.Expirations))
	for k := range d.Expirations {
		expKeys = append(expKeys, k)
	}
	sort.Strings(expKeys)
	exp := []map[string]any{}
	for _, k := range expKeys {
		exp = append(exp, map[string]any{"key_hex": hexOf([]byte(k)), "at": decI64(d.Expirations[k])})
	}
	ents := []map[string]any{}
	for _, ty := range d.Types {
		docs := []map[string]any{}
		for _, doc := range d.Entities[ty] {
			b, _ := json.Marshal(doc.Doc)
			docs = append(docs, map[string]any{"id_hex": hexOf([]byte(doc.ID)), "json": string(b)})
		}
		ents = append(ents, map[string]any{"entity_hex": hexOf([]byte(ty)), "documents": docs})
	}
	types := []string{}
	for _, ty := range d.Types {
		types = append(types, hexOf([]byte(ty)))
	}
	return map[string]any{"kv": kv, "expirations": exp, "types_hex": types, "entities": ents, "replayed": d.Replayed}
}

func legacyFailure(err error) map[string]any {
	msg := err.Error()
	file, _, _ := strings.Cut(msg, ": ")
	var se *json.SyntaxError
	var te *json.UnmarshalTypeError
	var ce base64.CorruptInputError
	switch {
	case errors.As(err, &ce):
		return map[string]any{"file": file, "wrapped": "base64", "offset": int64(ce)}
	case errors.As(err, &se):
		return map[string]any{"file": file, "wrapped": "json_syntax", "offset": se.Offset}
	case errors.As(err, &te):
		return map[string]any{"file": file, "wrapped": "json_type", "offset": te.Offset, "value": te.Value}
	}
	return map[string]any{"file": file, "wrapped": "other"}
}

func TestGenerateGoldenLegacy(t *testing.T) {
	dir := goldenSet(t, "legacy")
	now := time.Unix(1800000000, 0)
	for _, c := range legacyCases() {
		input := filepath.Join(dir, "read", c.name, "input")
		if err := os.MkdirAll(input, 0o755); err != nil {
			t.Fatal(err)
		}
		for name, content := range c.files {
			if err := os.WriteFile(filepath.Join(input, name), []byte(content), 0o644); err != nil {
				t.Fatal(err)
			}
		}
		out := map[string]any{"replay": c.replay, "now": 1800000000, "present": legacy.Present(input)}
		d, err := legacy.Read(input, c.replay, now)
		if err != nil {
			out["error"] = legacyFailure(err)
		} else {
			out["data"] = legacyDataJSON(d)
		}
		goldenJSON(t, filepath.Join(dir, "read", c.name, "expected.json"), out)
	}
	writes := []struct {
		name string
		d    *legacy.Data
	}{
		{"special", &legacy.Data{
			KV:          map[string][]byte{"plain": []byte("v"), "bin": {0, 0xff, 0x10}, "<&>": []byte("html"), "api:entity:skip": []byte("dropped"), "ttl": []byte("t")},
			Expirations: map[string]int64{"ttl": 4102444800, "absent": 5},
			Types:       []string{"book", "a\u2028b"},
			Entities: map[string][]legacy.Document{
				"book":     {{ID: "1", Doc: map[string]any{"title": "<b>&", "n": 1e21}}, {ID: "\u00e9", Doc: map[string]any{"x": nil}}},
				"a\u2028b": {{ID: "z", Doc: map[string]any{}}},
			},
		}},
		{"empty", &legacy.Data{KV: map[string][]byte{}, Expirations: map[string]int64{}, Entities: map[string][]legacy.Document{}}},
	}
	for _, w := range writes {
		target := filepath.Join(dir, "write", w.name)
		if err := legacy.Write(target, w.d); err != nil {
			t.Fatal(err)
		}
	}
}

func TestGenerateGoldenMigration(t *testing.T) {
	dir := goldenSet(t, "migration")
	b64 := func(s string) string { return base64.StdEncoding.EncodeToString([]byte(s)) }
	docs := `{"api:entity:num:id:big":{"v":1e21},"api:entity:num:id:small":{"v":1e-7},"api:entity:num:id:pow":{"v":1152921504606846976},` +
		`"api:entity:num:id:negzero":{"v":-0},"api:entity:num:id:tenth":{"v":0.1},"api:entity:num:id:int":{"v":123456789},` +
		`"api:entity:num:id:nested":{"o":{"a":[1,2.5,{"b":null}],"s":"<x>"},"arr":[[],[{}]]}}`
	kv := `{"api:entity:types:list":"` + b64("num") + `","api:entity:num:internal:index:id":"` + b64("big\nsmall\npow\nnegzero\ntenth\nint\nnested") + `","k":"` + b64("v") + `"}`
	input := filepath.Join(dir, "numbers", "input")
	if err := os.MkdirAll(input, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(input, legacy.JsonDataFile), []byte(docs), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(input, legacy.DataFile), []byte(kv), 0o644); err != nil {
		t.Fatal(err)
	}
	work := filepath.Join(t.TempDir(), "data")
	copyTree(t, input, work)
	e, err := Open(folderOptions(work))
	if err != nil {
		t.Fatal(err)
	}
	dump := folderDump(t, e)
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
	goldenJSON(t, filepath.Join(dir, "numbers", "expected.json"), dump)
}

func TestGenerateGoldenMessages(t *testing.T) {
	path := goldenFile(t, "messages.json")
	root := t.TempDir()
	var rows []map[string]any
	add := func(site string, err error, dir string, inner ...error) {
		row := map[string]any{"site": site}
		if err == nil {
			row["error"] = nil
		} else {
			row["error"] = describeMessage(err, dir, inner)
		}
		rows = append(rows, row)
	}

	locked := filepath.Join(root, "locked")
	e, err := Open(folderOptions(locked))
	if err != nil {
		t.Fatal(err)
	}
	_, err = Open(folderOptions(locked))
	add("second open of a locked folder", err, locked)
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}

	base := filepath.Join(root, "base")
	runWorkload(t, base)

	corruptBoth := filepath.Join(root, "corrupt-both")
	copyTree(t, base, corruptBoth)
	for _, name := range []string{durable.ManifestFile, durable.PreviousManifest} {
		if err := os.WriteFile(filepath.Join(corruptBoth, name), []byte(`{"format":`), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	_, err = Open(folderOptions(corruptBoth))
	_, parseErr := parseManifest([]byte(`{"format":`))
	add("MANIFEST and MANIFEST.prev both unreadable", err, corruptBoth, parseErr)

	badLive := filepath.Join(root, "bad-live")
	copyTree(t, base, badLive)
	rewriteManifest(t, badLive, func(m map[string]any) { m["live"] = map[string]any{"9x": 1} })
	_ = os.Remove(filepath.Join(badLive, durable.PreviousManifest))
	_, err = Open(folderOptions(badLive))
	add("live key 9x in MANIFEST", err, badLive)

	unknownIndex := filepath.Join(root, "unknown-index")
	copyTree(t, base, unknownIndex)
	rewriteManifest(t, unknownIndex, func(m map[string]any) {
		ents := m["entities"].([]any)
		first := ents[0].(map[string]any)
		idx, _ := first["indexes"].([]any)
		first["indexes"] = append(idx, map[string]any{"name": "zz", "root": map[string]any{"id": 0, "size": 0}})
	})
	_ = os.Remove(filepath.Join(unknownIndex, durable.PreviousManifest))
	_, err = Open(folderOptions(unknownIndex))
	add("unknown index name in MANIFEST", err, unknownIndex)

	pageFlip := filepath.Join(root, "page-checksum")
	copyTree(t, base, pageFlip)
	_ = os.Remove(filepath.Join(pageFlip, durable.PreviousManifest))
	flipRootPage(t, pageFlip)
	_, err = Open(folderOptions(pageFlip))
	add("page checksum mismatch", err, pageFlip)

	missingPages := filepath.Join(root, "missing-pages")
	copyTree(t, base, missingPages)
	_ = os.Remove(filepath.Join(missingPages, durable.PreviousManifest))
	for _, p := range globSorted(t, filepath.Join(missingPages, durable.PagesFolder, "*.pgs")) {
		_ = os.Remove(p)
	}
	_, err = Open(folderOptions(missingPages))
	add("missing page segment", err, missingPages)

	shortPages := filepath.Join(root, "short-pages")
	copyTree(t, base, shortPages)
	_ = os.Remove(filepath.Join(shortPages, durable.PreviousManifest))
	for _, p := range globSorted(t, filepath.Join(shortPages, durable.PagesFolder, "*.pgs")) {
		_ = os.Truncate(p, 20)
	}
	_, err = Open(folderOptions(shortPages))
	add("short page read", err, shortPages, rootPageErrors(t, shortPages)...)

	badKV := filepath.Join(root, "bad-kv")
	copyTree(t, base, badKV)
	_ = os.Remove(filepath.Join(badKV, durable.PreviousManifest))
	for _, p := range globSorted(t, filepath.Join(badKV, "kv-*.ckpt")) {
		flipMiddle(t, p)
	}
	_, err = Open(folderOptions(badKV))
	add("corrupt kv file", err, badKV)

	upgraded := filepath.Join(root, "format1-prev")
	copyTree(t, filepath.Join("..", "..", "test", "compat", "testdata", "pr-format1", "format1"), upgraded)
	e, err = Open(folderOptions(upgraded))
	if err != nil {
		t.Fatal(err)
	}
	e.Abandon()
	unreadable := filepath.Join(root, "format1-prev-unreadable")
	copyTree(t, upgraded, unreadable)
	_ = os.Remove(filepath.Join(upgraded, durable.ManifestFile))
	_, err = Open(folderOptions(upgraded))
	add("format-1 MANIFEST.prev after upgrade, MANIFEST removed", err, upgraded, checkpointOpenError(upgraded))
	if err := os.WriteFile(filepath.Join(unreadable, durable.ManifestFile), []byte("garbage"), 0o644); err != nil {
		t.Fatal(err)
	}
	_, err = Open(folderOptions(unreadable))
	_, garbageErr := parseManifest([]byte("garbage"))
	add("format-1 MANIFEST.prev after upgrade, MANIFEST unreadable", err, unreadable, garbageErr, checkpointOpenError(unreadable))

	jdir := filepath.Join(root, "journal")
	j, err := journal.Open(jdir, journal.Options{Mode: journal.ModeSync, SegmentSize: 4096})
	if err != nil {
		t.Fatal(err)
	}
	j.Start()
	recs := []journal.Record{{Kind: journal.KindKVPut, Key: "k", Value: []byte("value")}}
	ticket, err := j.Append(recs)
	if err != nil {
		t.Fatal(err)
	}
	if err := ticket.Wait(); err != nil {
		t.Fatal(err)
	}
	_ = os.Truncate(filepath.Join(jdir, journal.SegmentName(recs[0].Loc.Segment)), int64(recs[0].Loc.Offset)+2)
	_, err = j.ReadValue(recs[0].Loc)
	add("ReadValue short read", err, jdir)
	if err := j.Close(); err != nil {
		add("journal close", err, jdir)
	}
	_, err = j.Append(recs)
	add("append after close", err, jdir)

	bg, err := journal.Open(filepath.Join(root, "journal-bg"), journal.Options{Mode: journal.ModeBackground, SegmentSize: 4096})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := bg.Append([]journal.Record{{Kind: journal.KindKVPut, Key: "k", Value: []byte("v")}}); err != nil {
		t.Fatal(err)
	}
	add("close with unwritten data", bg.Close(), filepath.Join(root, "journal-bg"))

	goldenJSON(t, path, rows)
}

func globSorted(t *testing.T, pattern string) []string {
	t.Helper()
	out, err := filepath.Glob(pattern)
	if err != nil {
		t.Fatal(err)
	}
	sort.Strings(out)
	return out
}

func rewriteManifest(t *testing.T, dir string, fn func(m map[string]any)) {
	t.Helper()
	p := filepath.Join(dir, durable.ManifestFile)
	data, err := os.ReadFile(p)
	if err != nil {
		t.Fatal(err)
	}
	var m map[string]any
	if err := json.Unmarshal(data, &m); err != nil {
		t.Fatal(err)
	}
	fn(m)
	out, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(p, append(out, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}

func flipRootPage(t *testing.T, dir string) {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(dir, durable.ManifestFile))
	if err != nil {
		t.Fatal(err)
	}
	var m manifest
	if err := json.Unmarshal(data, &m); err != nil {
		t.Fatal(err)
	}
	seg, off := durable.SplitPageID(m.Entities[0].Slots.ID)
	p := filepath.Join(dir, durable.PagesFolder, fmt.Sprintf("%06d.pgs", seg))
	raw, err := os.ReadFile(p)
	if err != nil {
		t.Fatal(err)
	}
	raw[off+durable.PageHeaderSize+1] ^= 0x01
	if err := os.WriteFile(p, raw, 0o644); err != nil {
		t.Fatal(err)
	}
}

func goPart(err error) (string, string, bool) {
	for e := err; e != nil; e = errors.Unwrap(e) {
		switch e.(type) {
		case *json.SyntaxError, *json.UnmarshalTypeError:
			return e.Error(), "json", true
		case *fs.PathError, *os.LinkError, *os.SyscallError:
			return e.Error(), "file_system", true
		}
		if e == io.EOF {
			return e.Error(), "end_of_file", true
		}
		if e == io.ErrUnexpectedEOF {
			return e.Error(), "end_of_file", true
		}
	}
	return "", "", false
}

func describeMessage(err error, dir string, inner []error) map[string]any {
	msg := strings.ReplaceAll(err.Error(), dir, "<dir>")
	var kinds []string
	sources := append([]error{err}, inner...)
	for _, src := range sources {
		if src == nil {
			continue
		}
		text, kind, ok := goPart(src)
		if !ok {
			continue
		}
		text = strings.ReplaceAll(text, dir, "<dir>")
		if !strings.Contains(msg, text) {
			continue
		}
		if kind == "end_of_file" {
			if !strings.HasSuffix(msg, ": "+text) && msg != text {
				continue
			}
			msg = strings.TrimSuffix(msg, text) + "<go:" + kind + ">"
		} else {
			msg = strings.ReplaceAll(msg, text, "<go:"+kind+">")
		}
		kinds = append(kinds, kind)
	}
	if len(kinds) == 0 {
		return map[string]any{"message": msg}
	}
	return map[string]any{"text": msg, "wrapped": kinds}
}

func rootPageErrors(t *testing.T, dir string) []error {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(dir, durable.ManifestFile))
	if err != nil {
		t.Fatal(err)
	}
	m, err := parseManifest(data)
	if err != nil {
		t.Fatal(err)
	}
	store, err := durable.OpenPageStore(nil, filepath.Join(dir, durable.PagesFolder), 1<<30, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	var out []error
	for _, r := range m.roots() {
		if r.ID == 0 {
			continue
		}
		if _, err := store.Load(pagetree.StoredRef(r.ID, r.Size)); err != nil {
			out = append(out, err)
		}
	}
	return out
}

func checkpointOpenError(dir string) error {
	data, err := os.ReadFile(filepath.Join(dir, durable.PreviousManifest))
	if err != nil {
		return err
	}
	m, err := parseManifest(data)
	if err != nil || m.Checkpoint == "" {
		return err
	}
	_, err = readCheckpoint(filepath.Join(dir, m.Checkpoint))
	return err
}
