package datastore

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/docpath"
	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/pagetree"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func engineOut(t *testing.T) string {
	t.Helper()
	dir := filepath.Join(goldenRoot(t), "engine")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	return dir
}

func writeLines(t *testing.T, dir, name string, rows []any) {
	t.Helper()
	var l goldenio.Lines
	for _, r := range rows {
		l.Add(r)
	}
	if err := l.Write(dir, name); err != nil {
		t.Fatal(err)
	}
}

func bsl(s string) string {
	return strings.ReplaceAll(s, "~", "\\")
}

var handDocs = []string{
	`{}`,
	`{"a":null,"b":true,"c":false,"n":0,"s":"","arr":[],"obj":{}}`,
	`{"num":-0,"big":1e21,"small":1e-7,"huge":9007199254740993,"neg":-12.5,"exp":1E+300,"frac":0.1}`,
	`{"title":"Hello World","tags":["x","Y","x",1,true,null,{"k":"v"},[1,2]]}`,
	`{"date":"2026-11-23T18:39:01Z","d2":"2026-11-23","d3":"2026-11-23T18:39:01+01:00","d4":"2026-11-23 18:39:01","d5":"0000-01-01","d6":"9999-12-31T23:59:59.999999999Z","d7":"2026-02-30","d8":"2026-11-23T18:39:01,5Z","d9":"12345-01-01"}`,
	`{"nul":"a~u0000b","nul2":"~u0000","ff":"~u00ff"}`,
	`{"tr":"~u0130stanbul","sigma":"~u03a3~u03c3~u03c2","eszett":"~u1e9e","dz":"~u01c5","kelvin":"~u212a","mixed":"~u0130~u03a3~u1e9e~u01c5~u212a"}`,
	`{"author":{"@entity":"person","id":"p1"},"co":[{"@entity":"person","id":"p2"},{"@entity":"person","id":"p3"},"x"],"deep":{"inner":{"@entity":"org","id":"o1"}}}`,
	`{"@entity":"top","id":"self","title":"x"}`,
	`{"_elysiandb_core_owner":"admin","_elysiandb_core_x":{"y":1},"id":"i1","title":"core"}`,
	`{"id":42,"title":["a","b"],"a":{"b":{"c":"deep"}},"a.b":"dotted","a..b":"double"}`,
	`{"items":[{"name":"one","n":1},{"name":"Two","n":2},{"n":3},"loose"],"matrix":[[1,2],[3,[4]]]}`,
	`{"status":"open","num":3,"title":"T"}`,
	`{"status":["open","closed"],"num":[1,2],"title":"multi"}`,
	`{"status":{"x":1},"num":"7","title":null}`,
	`{"unicode":"~ud83d~ude00 emoji","lone":"~ud800","tab":"a~tb","quote":"~"q~"","html":"<a&b>","ls":"~u2028~u2029"}`,
	`{"title":"Ab","t2":"abc","t3":"ABCD","t4":"aaaa","t5":"~u00e9t~u00e9"}`,
	"{\"t6\":\"\xffbad\",\"t7\":\"\xc3\xa9t\xc3\xa9\"}",
	`{"tags":[]}`,
	`{"tags":"single"}`,
	`{"date":["2026-01-01","2027-01-01T00:00:00Z","nope"]}`,
	`{"num":1.7976931348623157e308,"min":5e-324,"m2":-1.7976931348623157e308}`,
	`{"bools":[true,false],"flag":true}`,
}

var engineWords = []string{"alpha", "Beta", "GAMMA", "delta", "\u00e9t\u00e9", "\u0130z", "\u03a3\u03c3", "x", "", "a b", "2026-01-01", "2026-06-15T10:00:00Z", "12", "-0", "null", "True"}

func randomValue(r *rand.Rand, depth int) any {
	switch r.Intn(9) {
	case 0:
		return nil
	case 1:
		return r.Intn(2) == 0
	case 2:
		return float64(r.Intn(2000) - 1000)
	case 3:
		return r.NormFloat64() * math.Pow(10, float64(r.Intn(30)-15))
	case 4, 5:
		return engineWords[r.Intn(len(engineWords))]
	case 6:
		return time.Unix(r.Int63n(4102444800)-2208988800, 0).UTC().Format(time.RFC3339)
	case 7:
		if depth > 2 {
			return "leaf"
		}
		n := r.Intn(4)
		arr := make([]any, n)
		for i := range arr {
			arr[i] = randomValue(r, depth+1)
		}
		return arr
	default:
		if depth > 2 {
			return 1.0
		}
		m := map[string]any{}
		for i := r.Intn(3); i > 0; i-- {
			m[[]string{"b", "name", "n", "c"}[r.Intn(4)]] = randomValue(r, depth+1)
		}
		return m
	}
}

func randomDoc(r *rand.Rand) map[string]any {
	doc := map[string]any{}
	for _, f := range []string{"title", "num", "date", "tags", "status", "a", "items", "flag"} {
		if r.Intn(4) == 0 {
			continue
		}
		switch f {
		case "num":
			doc[f] = float64(r.Intn(100))
		case "status":
			doc[f] = []string{"open", "closed", "draft", "Open"}[r.Intn(4)]
		case "items":
			n := r.Intn(4)
			items := make([]any, n)
			for i := range items {
				items[i] = map[string]any{"name": engineWords[r.Intn(len(engineWords))], "n": float64(i)}
			}
			doc[f] = items
		default:
			doc[f] = randomValue(r, 0)
		}
	}
	if r.Intn(5) == 0 {
		doc["author"] = map[string]any{"@entity": "person", "id": fmt.Sprintf("p%d", r.Intn(50))}
	}
	return doc
}

func engineCorpus() [][]byte {
	var out [][]byte
	for _, s := range handDocs {
		out = append(out, []byte(bsl(s)))
	}
	r := rand.New(rand.NewSource(1))
	for i := 0; i < 2000; i++ {
		b, err := json.Marshal(randomDoc(r))
		if err != nil {
			panic(err)
		}
		out = append(out, b)
	}
	return out
}

func hexList(bs [][]byte) []string {
	out := []string{}
	for _, b := range bs {
		out = append(out, hexOf(b))
	}
	return out
}

func hexStrings(ss []string) []string {
	out := []string{}
	for _, s := range ss {
		out = append(out, hexOf([]byte(s)))
	}
	return out
}

func sortValueKind(v any) map[string]any {
	switch x := v.(type) {
	case nil:
		return map[string]any{"kind": "nil"}
	case string:
		return map[string]any{"kind": "string"}
	case float64:
		return map[string]any{"kind": "number", "bits": fmt.Sprintf("%016x", math.Float64bits(x))}
	case bool:
		return map[string]any{"kind": "bool", "value": x}
	case time.Time:
		return map[string]any{"kind": "time", "unix": x.Unix(), "nanos": x.Nanosecond()}
	case map[string]any:
		return map[string]any{"kind": "object"}
	case []any:
		return map[string]any{"kind": "array"}
	}
	return map[string]any{"kind": fmt.Sprintf("%T", v)}
}

var indexFields = []string{"", "a", "a.b", "a..b", "a.b.c", "tags", "date", "num", "title", "status", "items.name", "items.n", "author", "missing", "missing.deep", "co", "matrix"}

var compositePairs = []string{"status,num", "title,date", "a,a.b", "tags,num", "status,missing"}

func TestGenerateEngineGolden(t *testing.T) {
	dir := engineOut(t)
	corpus := engineCorpus()
	var corpusRows []any
	for i, raw := range corpus {
		corpusRows = append(corpusRows, map[string]any{"n": i, "raw": base64.StdEncoding.EncodeToString(raw)})
	}
	writeLines(t, dir, "corpus.jsonl", corpusRows)

	var keyRows []any
	for i, raw := range corpus {
		doc := decodeDoc(raw)
		fields := map[string]any{}
		for _, f := range indexFields {
			entry := map[string]any{}
			if k := FilterKeys(doc, f); len(k) > 0 {
				entry["filter"] = hexList(k)
			}
			unique := map[string]any{}
			for _, kind := range []IndexKind{IndexFilter, IndexSort, IndexRefs} {
				if u := uniqueKeys(extractKeys(IndexSpec{Kind: kind, Field: f}, doc)); len(u) > 0 {
					unique[string(kind)] = hexStrings(u)
				}
			}
			if len(unique) > 0 {
				entry["unique"] = unique
			}
			entry["sort"] = hexOf(SortKey(doc, f))
			entry["sort_value"] = sortValueKind(docpath.SortValue(doc, f))
			fields[f] = entry
		}
		row := map[string]any{"doc": i, "decoded": doc != nil, "fields": fields}
		row["refs"] = hexList(RefKeys(doc))
		refs := DocumentRefs(doc)
		var pairs [][]string
		for _, r := range refs {
			pairs = append(pairs, []string{hexOf([]byte(r.Path)), hexOf([]byte(r.Target))})
		}
		sort.Slice(pairs, func(a, b int) bool {
			if pairs[a][0] != pairs[b][0] {
				return pairs[a][0] < pairs[b][0]
			}
			return pairs[a][1] < pairs[b][1]
		})
		row["doc_refs"] = pairs
		var comps []any
		for _, c := range compositePairs {
			keys, irregular := compositeKeys(IndexSpec{Kind: IndexComposite, Field: c}, doc)
			comps = append(comps, map[string]any{"spec": c, "keys": hexStrings(keys), "irregular": irregular})
		}
		row["composite"] = comps
		grams := append([]uint32(nil), searchGrams(doc)...)
		sort.Slice(grams, func(a, b int) bool { return grams[a] < grams[b] })
		row["grams"] = grams
		var tri []string
		for k := range DocumentTrigrams(doc) {
			tri = append(tri, k)
		}
		sort.Strings(tri)
		row["trigrams"] = hexStrings(tri)
		elems := map[string]any{}
		for k, v := range doc {
			if arr, ok := v.([]any); ok {
				var out []string
				for _, item := range arr {
					out = append(out, hexOf([]byte(ElementString(item))))
				}
				elems[hexOf([]byte(k))] = out
			}
		}
		row["elements"] = elems
		keyRows = append(keyRows, row)
	}
	writeLines(t, dir, "index_keys.jsonl", keyRows)

	writeLines(t, dir, "primitives.jsonl", enginePrimitives())
	writeLines(t, dir, "strings.jsonl", engineStrings())
	writeLines(t, dir, "postings.jsonl", enginePostings())
	writeLines(t, dir, "put_bytes.jsonl", enginePutBytes(corpus))
	engineScenario(t, dir)
}

func enginePrimitives() []any {
	var rows []any
	floats := []float64{0, math.Copysign(0, -1), 1, -1, 0.5, -0.5, math.SmallestNonzeroFloat64, -math.SmallestNonzeroFloat64, math.Float64frombits(0x000fffffffffffff), math.MaxFloat64, -math.MaxFloat64,
		math.Inf(1), math.Inf(-1), 1 << 53, 1<<53 + 2, -(1 << 53), 9007199254740991, 1e21, 1e-7, 123.456, -123.456}
	for _, f := range floats {
		rows = append(rows, map[string]any{"fn": "AppendNumber", "in": fmt.Sprintf("%016x", math.Float64bits(f)), "out": hexOf(AppendNumber(nil, f))})
	}
	times := []struct{ s, ns int64 }{{0, 0}, {-1, 0}, {-1, 999999999}, {1700000000, 123456789}, {-62167219200, 0}, {253402300799, 999999999}, {-62135596800, 1}, {1 << 40, 5}}
	for _, tt := range times {
		rows = append(rows, map[string]any{"fn": "AppendTime", "in": map[string]any{"unix": tt.s, "nanos": tt.ns}, "out": hexOf(AppendTime(nil, time.Unix(tt.s, tt.ns)))})
	}
	for _, s := range []string{"", "a", "\x00", "\xff", "\x00\xff", "a\x00b", "\x00\x00", "\xff\x00\x01"} {
		rows = append(rows, map[string]any{"fn": "AppendEscaped", "in": hexOf([]byte(s)), "out": hexOf(AppendEscaped(nil, s))})
		rows = append(rows, map[string]any{"fn": "AppendTerminated", "in": hexOf([]byte(s)), "out": hexOf(AppendTerminated(nil, s))})
		rows = append(rows, map[string]any{"fn": "CompositePrefix", "in": hexOf([]byte(s)), "out": hexOf(CompositePrefix(s))})
	}
	values := []struct {
		name string
		v    any
	}{
		{"nil", nil}, {"float64", 2.5}, {"float32", float32(2.5)}, {"int", 7}, {"int32", int32(-7)}, {"int64", int64(1 << 40)}, {"time", time.Unix(1700000000, 5)},
		{"string", "a\x00b"}, {"true", true}, {"false", false}, {"map", map[string]any{}}, {"slice", []any{1.0}}, {"uint", uint(3)},
	}
	for _, v := range values {
		rows = append(rows, map[string]any{"fn": "SortValueKey", "in": v.name, "out": hexOf(SortValueKey(v.v))})
	}
	for _, p := range [][]byte{nil, {}, {0}, {0xff}, {1, 0xff}, {0xff, 0xff}, {1, 2, 3}, {0x41, 0xff, 0xff}} {
		end := prefixEnd(p)
		row := map[string]any{"fn": "prefixEnd", "in": hexOf(p)}
		if end == nil {
			row["out"] = nil
		} else {
			row["out"] = hexOf(end)
		}
		rows = append(rows, row)
	}
	for _, s := range []uint32{0, 1, 255, 256, 1<<24 + 3, math.MaxUint32} {
		rows = append(rows, map[string]any{"fn": "seqKey", "in": s, "out": hexOf([]byte(seqKey(s)))})
		rows = append(rows, map[string]any{"fn": "postingKey", "in": map[string]any{"gram": s & 0xffffff, "chunk": s >> 8}, "out": hexOf([]byte(postingKey(s&0xffffff, s>>8)))})
		rows = append(rows, map[string]any{"fn": "bitmapKey", "in": map[string]any{"value": "v\x00", "chunk": s}, "out": hexOf([]byte(bitmapKey("v\x00", s)))})
	}
	slots := []slot{
		{seq: 1, id: "a", lsn: 1, loc: journal.Location{Segment: 1, Offset: 16, Length: 3}},
		{seq: 1 << 20, id: "", lsn: 1 << 40, loc: journal.Location{Segment: math.MaxUint32, Offset: math.MaxUint32, Length: 0}},
		{seq: 7, id: "\xff\x00id", lsn: 0, loc: journal.Location{}},
	}
	for _, s := range slots {
		enc := encodeSlot(nil, s)
		for _, shared := range []bool{false, true} {
			d := decodeSlotValue(s.seq, enc, shared)
			rows = append(rows, map[string]any{"fn": "encodeSlot", "in": map[string]any{"seq": s.seq, "id": hexOf([]byte(s.id)), "lsn": dec64(s.lsn), "segment": s.loc.Segment, "offset": s.loc.Offset, "length": s.loc.Length, "shared": shared},
				"out": hexOf(enc), "decoded": map[string]any{"seq": d.seq, "id": hexOf([]byte(d.id)), "lsn": dec64(d.lsn), "segment": d.loc.Segment, "offset": d.loc.Offset, "length": d.loc.Length}})
		}
	}
	for _, b := range [][]byte{nil, {0x80}, {0x01, 0x80}, {0x01, 0x02, 0x03}, {0x01, 0x01, 0x10, 0x03, 0x80, 0x80}, {0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01}} {
		d := decodeSlotValue(9, b, false)
		rows = append(rows, map[string]any{"fn": "decodeSlotValue", "in": hexOf(b), "decoded": map[string]any{"seq": d.seq, "id": hexOf([]byte(d.id)), "lsn": dec64(d.lsn), "segment": d.loc.Segment, "offset": d.loc.Offset, "length": d.loc.Length}})
	}
	tree := pagetree.New(nil).WithToken(pagetree.NewToken())
	for _, delta := range []int64{1, 1, -3, 64, -63, 0, 1 << 40, -(1 << 40)} {
		tree = addIrregular(tree, delta)
		v, ok := tree.Get(irregularKey)
		rows = append(rows, map[string]any{"fn": "addIrregular", "in": delta, "count": irregularCount(tree), "present": ok, "stored": hexOf(v)})
	}
	return rows
}

func engineStrings() []any {
	var rows []any
	vals := []any{0.0, math.Copysign(0, -1), 1.0, -1.5, 1e20, 1e21, 1e-6, 1e-7, 123456789.0, 0.1, math.MaxFloat64, math.SmallestNonzeroFloat64, float64(1 << 53), float64(1<<53 + 2), math.Inf(1), math.NaN()}
	for _, v := range vals {
		f := v.(float64)
		rows = append(rows, map[string]any{"fn": "FormatFloat_f", "in": fmt.Sprintf("%016x", math.Float64bits(f)), "out": strconv.FormatFloat(f, 'f', -1, 64)})
		rows = append(rows, map[string]any{"fn": "Sprintf_v", "in": fmt.Sprintf("%016x", math.Float64bits(f)), "out": fmt.Sprintf("%v", f)})
		rows = append(rows, map[string]any{"fn": "ElementString", "in": fmt.Sprintf("%016x", math.Float64bits(f)), "out": ElementString(f)})
	}
	nested := []string{`[1,"a",null,true,[2,[3]]]`, `{"b":1,"a":[1,2],"c":{"z":null,"y":"s"}}`, `[]`, `{}`, `[{"k":1e21}]`, `"str"`, `true`, `null`}
	for _, s := range nested {
		var v any
		if err := json.Unmarshal([]byte(s), &v); err != nil {
			panic(err)
		}
		rows = append(rows, map[string]any{"fn": "ElementString_json", "in": s, "out": hexOf([]byte(ElementString(v)))})
	}
	rows = append(rows, map[string]any{"fn": "CompositePrefix_rule", "absent_blocks": "unchanged by lowering"})
	for start := rune(0); start <= 0x10ffff; start += 128 {
		if start >= 0xd800 && start < 0xe000 {
			continue
		}
		var b strings.Builder
		for c := start; c < start+128 && c <= 0x10ffff; c++ {
			b.WriteRune(c)
		}
		block := b.String()
		got := CompositePrefix(block)
		if string(got) == string(AppendTerminated([]byte{KeyLower}, block)) {
			continue
		}
		rows = append(rows, map[string]any{"fn": "CompositePrefix_block", "start": start, "out": hexOf(got)})
	}
	for _, s := range []string{"", "ABC", "a\xffB", "\xc0\xafX", "\xed\xa0\x80Y", "MiX\x00ed", "\u0130\u212a\xff", "\xf4\x90\x80\x80Z"} {
		rows = append(rows, map[string]any{"fn": "lowerKey", "in": hexOf([]byte(s)), "out": hexOf([]byte(lowerKey(s)))})
	}
	for _, p := range []string{"", "ab", "abc", "abcd", "*abc*", "a*bcd*ef", "ABC", "\u00e9t\u00e9", "a?c", "x\\*y", "***", "\xffab", "aaaa"} {
		tri := PatternTrigrams(p)
		sorted := append([]string(nil), tri...)
		sort.Strings(sorted)
		rows = append(rows, map[string]any{"fn": "PatternTrigrams", "in": hexOf([]byte(p)), "out": hexStrings(sorted)})
	}
	globs := [][2]string{
		{"", ""}, {"", "a"}, {"*", ""}, {"*", "abc"}, {"a*", "abc"}, {"*c", "abc"}, {"a*c", "abbbc"}, {"a*c", "abcd"}, {"**", "x"}, {"***", ""},
		{"a\\*b", "a*b"}, {"a\\*b", "axb"}, {"a\\", "a\\"}, {"a\\", "a"}, {"\\\\", "\\"}, {"ABC", "abc"}, {"\u00c9T\u00c9", "\u00e9t\u00e9"}, {"\xff*", "\xffz"},
		{"*\u0130*", "xi\u0307y"}, {"?", "a"}, {"a*b*c", "aXbYc"}, {"a*b*c", "aXcYb"}, {"*a", "aaaa"}, {"k*", "\u212a"},
	}
	for _, g := range globs {
		rows = append(rows, map[string]any{"fn": "MatchGlob", "pattern": hexOf([]byte(g[0])), "subject": hexOf([]byte(g[1])), "out": docpath.MatchGlob(g[0], g[1])})
	}
	return rows
}

func enginePostings() []any {
	var rows []any
	script := func(name string, ops [][2]int) {
		var c []byte
		var steps []string
		for _, op := range ops {
			if op[0] > 0 {
				c = postingAdd(c, uint16(op[1]))
			} else {
				c = postingRemove(c, uint16(op[1]))
			}
			steps = append(steps, hexOf(c))
		}
		var each []uint32
		postingEach(c, 1<<postingChunkBits, func(seq uint32) { each = append(each, seq) })
		rows = append(rows, map[string]any{"script": name, "ops": ops, "steps": steps, "count": postingCount(c), "each_base_chunk1": each})
	}
	var grow [][2]int
	for i := 0; i < 300; i++ {
		grow = append(grow, [2]int{1, (i * 37) % 4096})
	}
	script("grow-to-bitmap", grow)
	var down [][2]int
	down = append(down, grow...)
	for i := 0; i < 300; i++ {
		down = append(down, [2]int{-1, (i * 37) % 4096})
	}
	script("bitmap-down-to-empty", down)
	script("duplicates-and-missing", [][2]int{{1, 5}, {1, 5}, {-1, 9}, {1, 0}, {1, 65535}, {-1, 5}, {-1, 5}, {-1, 0}, {-1, 65535}})
	script("array-order", [][2]int{{1, 9}, {1, 3}, {1, 7}, {1, 1}, {-1, 7}, {1, 8}})
	mk := func(offs ...int) []byte {
		var c []byte
		for _, o := range offs {
			c = postingAdd(c, uint16(o))
		}
		return c
	}
	var big []int
	for i := 0; i < 400; i++ {
		big = append(big, i*3)
	}
	containers := map[string][]byte{"small": mk(1, 2, 3, 500), "other": mk(2, 3, 4, 1000), "big": mk(big...), "empty": nil}
	names := []string{"big", "empty", "other", "small"}
	for _, x := range names {
		for _, y := range names {
			for _, op := range []string{"And", "AndNot", "Or"} {
				a := NewBitset(1 << 17)
				a.addContainer(0, containers[x])
				a.addContainer(1, containers[y])
				b := NewBitset(1 << 17)
				b.addContainer(0, containers[y])
				b.addContainer(1, containers[x])
				switch op {
				case "And":
					a.And(b)
				case "AndNot":
					a.AndNot(b)
				case "Or":
					a.Or(b)
				}
				var bits []uint32
				a.ForEach(func(i uint32) bool { bits = append(bits, i); return true })
				rows = append(rows, map[string]any{"script": "bitset", "op": op, "a": []string{x, y}, "b": []string{y, x}, "count": a.Count(), "bits": bits})
			}
		}
	}
	for _, n := range []string{"big", "other", "small"} {
		rows = append(rows, map[string]any{"script": "container", "name": n, "hex": hexOf(containers[n])})
	}
	opsCases := [][]indexOp{
		{{key: "b", seq: 1, kind: opPut}, {key: "a", seq: 2, kind: opPut}, {key: "b", seq: 3, kind: opDelete}, {key: "a", seq: 4, kind: opDelete}, {key: "a", seq: 5, kind: opPut}},
		{{key: "k", seq: 1, kind: opBitmapAdd}, {key: "k", seq: 2, kind: opBitmapAdd}, {key: "k", seq: 1, kind: opBitmapRemove}, {key: "j", seq: 3, kind: opBitmapRemove}, {key: "k", seq: 2, kind: opBitmapRemove}},
		{{key: "z", seq: 9, kind: opPut}, {key: "z", seq: 9, kind: opPut}, {key: "y", seq: 1, kind: opBitmapAdd}, {key: "z", seq: 9, kind: opDelete}},
	}
	for i, ops := range opsCases {
		var in []any
		for _, o := range ops {
			in = append(in, map[string]any{"key": hexOf([]byte(o.key)), "seq": o.seq, "kind": int(o.kind)})
		}
		tree := applyOps(pagetree.New(nil).WithToken(pagetree.NewToken()), append([]indexOp(nil), ops...))
		var dump [][]string
		tree.Scan(func(k string, v []byte) bool {
			dump = append(dump, []string{hexOf([]byte(k)), hexOf(v)})
			return true
		})
		rows = append(rows, map[string]any{"script": fmt.Sprintf("applyOps-%d", i), "ops": in, "steps": []string{}, "dump": dump})
	}
	return rows
}

func enginePutBytes(corpus [][]byte) []any {
	var rows []any
	for i, raw := range corpus {
		if i >= len(handDocs) && i%20 != 0 {
			continue
		}
		doc := decodeDoc(raw)
		if doc == nil {
			rows = append(rows, map[string]any{"in": fmt.Sprintf("corpus:%d", i), "site": "decodeDoc", "error": true})
			continue
		}
		b, err := json.Marshal(doc)
		if err != nil {
			rows = append(rows, map[string]any{"in": fmt.Sprintf("corpus:%d", i), "site": "Tx.Put", "error": true})
			continue
		}
		rows = append(rows, map[string]any{"in": fmt.Sprintf("corpus:%d", i), "bytes": base64.StdEncoding.EncodeToString(b), "decoded_ok": decodeDoc(b) != nil})
	}
	goValues := []struct {
		name string
		doc  map[string]any
	}{
		{"int", map[string]any{"v": 7}},
		{"int64", map[string]any{"v": int64(math.MaxInt64)}},
		{"strings", map[string]any{"v": []string{"a", "<b>"}}},
		{"negzero", map[string]any{"v": math.Copysign(0, -1)}},
		{"html", map[string]any{"v": "<script>&amp;</script>"}},
		{"separators", map[string]any{"v": "\u2028\u2029"}},
		{"invalid", map[string]any{"v": "\xff\xfe", "\xffkey": 1.0}},
		{"nested", map[string]any{"a": map[string]any{"b": []any{map[string]any{"c": nil}}}}},
		{"nan", map[string]any{"v": math.NaN()}},
		{"inf", map[string]any{"v": math.Inf(1)}},
		{"uint8s", map[string]any{"v": []byte("raw")}},
	}
	for _, g := range goValues {
		b, err := json.Marshal(g.doc)
		if err != nil {
			rows = append(rows, map[string]any{"in": "go:" + g.name, "site": "Tx.Put", "error": true})
			continue
		}
		rows = append(rows, map[string]any{"in": "go:" + g.name, "bytes": base64.StdEncoding.EncodeToString(b), "decoded_ok": decodeDoc(b) != nil})
	}
	raws := []string{`{"a":1,"a":2}`, `null`, `{"v":1e400}`, `[1,2]`, `  {"a" : 1 }  `, `{"a":1} x`, "{\"a\":\"\xff\"}", `{"a":"~ud800"}`, `{"a":"~ud800~udc00"}`, ``, `{`, `"s"`, `{"a":-0}`, `{"a":01}`}
	for _, s := range raws {
		in := bsl(s)
		doc := decodeDoc([]byte(in))
		row := map[string]any{"in": "raw:" + hexOf([]byte(in)), "decoded_ok": doc != nil}
		if doc != nil {
			b, _ := json.Marshal(doc)
			row["bytes"] = base64.StdEncoding.EncodeToString(b)
		} else {
			row["site"] = "decodeDoc"
			row["error"] = true
		}
		rows = append(rows, row)
	}
	return rows
}
