package datastore

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"path/filepath"
	"reflect"
	"sort"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/journal"
)

type scenarioOp struct {
	Op     string         `json:"op"`
	Entity string         `json:"entity,omitempty"`
	ID     string         `json:"id,omitempty"`
	Doc    map[string]any `json:"doc,omitempty"`
	Index  string         `json:"index,omitempty"`
	Field  string         `json:"field,omitempty"`
	Step   string         `json:"step,omitempty"`
}

func scenarioDoc(i int) map[string]any {
	doc := map[string]any{
		"title":  fmt.Sprintf("Title %02d", i%13),
		"num":    float64(i % 17),
		"status": []string{"open", "closed", "draft"}[i%3],
		"tags":   []any{"red", "green", "blue"}[:i%4],
		"note":   fmt.Sprintf("searchable text %03d", i),
	}
	if i%4 == 1 {
		doc["author"] = map[string]any{"@entity": "people", "id": fmt.Sprintf("p%02d", i%7)}
	}
	return doc
}

func scenarioScript() []scenarioOp {
	var ops []scenarioOp
	add := func(o scenarioOp) { ops = append(ops, o) }
	add(scenarioOp{Op: "ensure_type", Entity: "s"})
	add(scenarioOp{Op: "ensure_type", Entity: "people"})
	add(scenarioOp{Op: "ensure_index", Entity: "s", Index: "f:title"})
	add(scenarioOp{Op: "ensure_index", Entity: "s", Index: "g:note"})
	for i := 0; i < 120; i++ {
		add(scenarioOp{Op: "put", Entity: "s", ID: fmt.Sprintf("d%03d", i), Doc: scenarioDoc(i)})
	}
	for i := 0; i < 7; i++ {
		add(scenarioOp{Op: "put", Entity: "people", ID: fmt.Sprintf("p%02d", i), Doc: map[string]any{"name": fmt.Sprintf("Person %d", i)}})
	}
	add(scenarioOp{Op: "dump", Step: "after-first-writes"})
	for _, idx := range []string{"s:num", "r:author", "b:status", "c:status,num", "f:tags"} {
		add(scenarioOp{Op: "ensure_index", Entity: "s", Index: idx})
	}
	add(scenarioOp{Op: "watch_field", Entity: "s", Field: "status"})
	add(scenarioOp{Op: "dump", Step: "after-indexes"})
	for i := 0; i < 120; i += 5 {
		add(scenarioOp{Op: "put", Entity: "s", ID: fmt.Sprintf("d%03d", i), Doc: scenarioDoc(i + 1)})
	}
	for i := 0; i < 120; i += 9 {
		add(scenarioOp{Op: "delete", Entity: "s", ID: fmt.Sprintf("d%03d", i)})
	}
	add(scenarioOp{Op: "put", Entity: "s", ID: "d009", Doc: scenarioDoc(500)})
	add(scenarioOp{Op: "dump", Step: "after-updates"})
	add(scenarioOp{Op: "reset_entity", Entity: "people"})
	add(scenarioOp{Op: "put", Entity: "people", ID: "p00", Doc: map[string]any{"name": "Again"}})
	add(scenarioOp{Op: "ensure_type", Entity: "gone"})
	add(scenarioOp{Op: "put", Entity: "gone", ID: "g", Doc: map[string]any{"x": 1.0}})
	add(scenarioOp{Op: "drop_type", Entity: "gone"})
	add(scenarioOp{Op: "dump", Step: "after-reset-and-drop"})
	add(scenarioOp{Op: "clear_all"})
	add(scenarioOp{Op: "put", Entity: "s", ID: "after-clear", Doc: scenarioDoc(1)})
	add(scenarioOp{Op: "ensure_index", Entity: "s", Index: "f:title"})
	add(scenarioOp{Op: "dump", Step: "after-clear"})
	return ops
}

func roundTripDoc(doc map[string]any) map[string]any {
	b, err := json.Marshal(doc)
	if err != nil {
		panic(err)
	}
	return decodeDoc(b)
}

func waitIndexes(t *testing.T, e *Engine) {
	t.Helper()
	deadline := time.Now().Add(30 * time.Second)
	for {
		busy := false
		for _, st := range e.IndexStates() {
			if st.State != "ready" && st.State != "dormant" {
				busy = true
			}
		}
		if !busy && e.IndexBacklog() == 0 {
			return
		}
		if time.Now().After(deadline) {
			t.Fatal("indexes did not settle")
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func scenarioDump(t *testing.T, e *Engine, step string, indexed bool, lsns []uint64) []map[string]any {
	t.Helper()
	var rows []map[string]any
	put := func(entity, what string, v any) {
		rows = append(rows, map[string]any{"step": step, "entity": entity, "what": what, "value": v})
	}
	put("", "types", hexStrings(e.Types()))
	names := append([]string(nil), e.Types()...)
	sort.Strings(names)
	for _, name := range names {
		var s *Snapshot
		if indexed {
			s = e.IndexedSnapshot(name)
		} else {
			s = e.Snapshot(name)
		}
		if s == nil {
			put(name, "snapshot", nil)
			continue
		}
		var docs [][]any
		s.Ascend(0, func(d Doc) bool {
			raw, err := d.Bytes()
			if err != nil {
				t.Fatal(err)
			}
			docs = append(docs, []any{d.Seq(), hexOf([]byte(d.ID())), dec64(d.LSN()), base64.StdEncoding.EncodeToString(raw)})
			return true
		})
		put(name, "docs", docs)
		var refs [][]string
		for _, r := range s.Refs() {
			refs = append(refs, []string{r.Path, r.Target})
		}
		put(name, "refs", refs)
		put(name, "counters", map[string]any{"version": dec64(s.Version()), "generation": dec64(s.Generation()), "next_seq": s.NextSeq(), "len": s.Len()})
		put(name, "index_names", e.IndexNames(name))
		for _, iname := range e.IndexNames(name) {
			spec, ok := ParseIndexName(iname)
			if !ok {
				continue
			}
			v, ok := s.Index(spec)
			if !ok {
				put(name, "index:"+iname, nil)
				continue
			}
			var entries [][]any
			v.ScanRange(nil, nil, func(key string, seq uint32) bool {
				entries = append(entries, []any{hexOf([]byte(key)), seq})
				return true
			})
			info := map[string]any{"entries": entries, "len": v.Len()}
			if spec.Kind == IndexBitmap {
				var ranks []any
				for _, p := range []string{"", "lclosed", "ldraft", "lopen", "lzzz"} {
					ranks = append(ranks, []any{p, v.Rank(AppendTerminated(nil, p))})
				}
				info["ranks"] = ranks
				set := NewBitset(1 << 12)
				v.Collect(nil, nil, set, nil)
				var bits []uint32
				set.ForEach(func(i uint32) bool { bits = append(bits, i); return true })
				info["collect_all"] = bits
				info["distinct"] = []int{v.DistinctValues(1), v.DistinctValues(2), v.DistinctValues(100)}
			}
			if spec.Kind == IndexComposite {
				info["irregular"] = v.Irregular()
				var subs []any
				for _, status := range []string{"open", "closed", "draft", "none"} {
					sub := v.Sub(CompositePrefix(status))
					var se [][]any
					sub.ScanRange(nil, nil, func(key string, seq uint32) bool {
						se = append(se, []any{hexOf([]byte(key)), seq})
						return true
					})
					subs = append(subs, map[string]any{"status": status, "len": sub.Len(), "entries": se})
				}
				info["subs"] = subs
			}
			put(name, "index:"+iname, info)
		}
		var search []any
		for _, p := range []string{"*text*", "*text 01*", "*xyz*", "ab", "*searchable*"} {
			set, ok := e.SearchCandidates(name, p)
			var bits []uint32
			if set != nil {
				set.ForEach(func(i uint32) bool { bits = append(bits, i); return true })
			}
			search = append(search, map[string]any{"pattern": p, "ok": ok, "seqs": bits})
		}
		put(name, "search_candidates", search)
		var changed []any
		for _, l := range lsns {
			seqs, ok := s.ChangedSince(l)
			sorted := append([]uint32(nil), seqs...)
			sort.Slice(sorted, func(a, b int) bool { return sorted[a] < sorted[b] })
			changed = append(changed, map[string]any{"lsn": dec64(l), "ok": ok, "seqs": sorted})
		}
		put(name, "changed_since", changed)
		var fv []any
		for _, terms := range [][]string{{"open"}, {"closed", "draft"}, {"none"}} {
			var tb [][]byte
			for _, term := range terms {
				tb = append(tb, []byte(term))
			}
			v, ok := s.FieldVersion("status", tb)
			fv = append(fv, map[string]any{"terms": terms, "version": dec64(v), "ok": ok})
		}
		put(name, "field_version", fv)
	}
	var states []map[string]any
	for _, st := range e.IndexStates() {
		states = append(states, map[string]any{"entity": st.Entity, "index": st.Index, "kind": st.Kind, "origin": st.Origin, "state": st.State, "pinned": st.Pinned, "entries": st.Entries})
	}
	sort.Slice(states, func(i, j int) bool {
		if states[i]["entity"].(string) != states[j]["entity"].(string) {
			return states[i]["entity"].(string) < states[j]["entity"].(string)
		}
		return states[i]["index"].(string) < states[j]["index"].(string)
	})
	put("", "index_states", states)
	return rows
}

func runScenario(t *testing.T, background bool) []map[string]any {
	t.Helper()
	dir := filepath.Join(t.TempDir(), "data")
	e, err := Open(Options{Folder: dir, Shards: 8, Durability: journal.ModeSync, Background: background, IndexWorkers: 1})
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()
	var rows []map[string]any
	lsns := []uint64{0, 1, 50, 130, 160, 1 << 40}
	for _, o := range scenarioScript() {
		switch o.Op {
		case "ensure_type":
			mustUpdate(t, e, func(tx *Tx) error { tx.EnsureType(o.Entity); return nil })
		case "drop_type":
			mustUpdate(t, e, func(tx *Tx) error { tx.DropType(o.Entity); return nil })
		case "reset_entity":
			mustUpdate(t, e, func(tx *Tx) error { tx.ResetEntity(o.Entity); return nil })
		case "clear_all":
			mustUpdate(t, e, func(tx *Tx) error { tx.ClearAll(); return nil })
		case "put":
			doc := roundTripDoc(o.Doc)
			mustUpdate(t, e, func(tx *Tx) error { return tx.Put(o.Entity, o.ID, doc) })
		case "delete":
			mustUpdate(t, e, func(tx *Tx) error { tx.Delete(o.Entity, o.ID); return nil })
		case "ensure_index":
			spec, ok := ParseIndexName(o.Index)
			if !ok {
				t.Fatalf("bad index %s", o.Index)
			}
			e.EnsureIndex(o.Entity, spec)
		case "watch_field":
			e.WatchField(o.Entity, o.Field)
		case "dump":
			if background {
				waitIndexes(t, e)
			}
			rows = append(rows, scenarioDump(t, e, o.Step, background, lsns)...)
		}
	}
	return rows
}

func engineScenario(t *testing.T, dir string) {
	t.Helper()
	var opsRows []any
	for _, o := range scenarioScript() {
		opsRows = append(opsRows, o)
	}
	writeLines(t, dir, "scenario_ops.jsonl", opsRows)
	plain := runScenario(t, false)
	bg := runScenario(t, true)
	a, _ := json.Marshal(plain)
	b, _ := json.Marshal(bg)
	if string(a) != string(b) {
		for i := range plain {
			if i >= len(bg) || !reflect.DeepEqual(plain[i], bg[i]) {
				x, _ := json.Marshal(plain[i])
				var y []byte
				if i < len(bg) {
					y, _ = json.Marshal(bg[i])
				}
				t.Fatalf("background run differs at row %d:\n%s\n%s", i, x, y)
			}
		}
		t.Fatal("background run differs")
	}
	var rows []any
	for _, r := range plain {
		rows = append(rows, r)
	}
	writeLines(t, dir, "scenario.jsonl", rows)
}
