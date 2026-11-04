package datastore_test

import (
	"slices"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
)

func TestIndexStatesNamesAndDrops(t *testing.T) {
	e := openBackground(t)
	putDocs(t, e, "states", 0, 3000, workDoc)

	sortA := datastore.IndexSpec{Kind: datastore.IndexSort, Field: "a"}
	filterB := datastore.IndexSpec{Kind: datastore.IndexFilter, Field: "b"}
	refs := datastore.IndexSpec{Kind: datastore.IndexRefs}
	for _, spec := range []datastore.IndexSpec{sortA, filterB, refs} {
		e.EnsureIndex("states", spec)
	}
	e.SearchCandidates("states", "*alpha*")
	waitReady(t, e, "states", sortA, filterB, refs)

	names := e.IndexNames("states")
	for _, spec := range []datastore.IndexSpec{sortA, filterB, refs} {
		if !slices.Contains(names, spec.Name()) {
			t.Fatalf("%s is listed, got %v", spec.Name(), names)
		}
	}
	if e.IndexNames("missing") != nil {
		t.Fatalf("an unknown entity has no index")
	}

	states := map[string]datastore.IndexState{}
	for _, st := range e.IndexStates() {
		if st.Entity == "states" {
			states[st.Index] = st
		}
	}

	if st := states[sortA.Name()]; st.State != "ready" || st.Entries != 3000 || st.LastUsed.IsZero() {
		t.Fatalf("a ready sort index has one entry per document, got %+v", st)
	}
	if st := states["search"]; st.State != "ready" || st.Entries != 3000 {
		t.Fatalf("the search index is named search and counts the documents it holds, got %+v", st)
	}
	if st := states[filterB.Name()]; st.State != "ready" || st.Entries != 3*3000 {
		t.Fatalf("a filter index holds the presence, the lowercase and the raw keys, got %+v", st)
	}

	view, _ := e.Snapshot("states").Index(filterB)
	if view.CountKind(datastore.KeyPresent) != 3000 || view.CountKind(datastore.KeyLower) != 3000 || view.CountBelow(datastore.KeyLower) != 0 {
		t.Fatalf("the kind counts follow the entries: present %d, lower %d", view.CountKind(datastore.KeyPresent), view.CountKind(datastore.KeyLower))
	}

	if n := e.DropIdleIndexes(-time.Minute); n != 4 {
		t.Fatalf("every ready index is released, got %d", n)
	}
	for _, st := range e.IndexStates() {
		if st.Entity == "states" && (st.State != "released" || st.Entries != 0) {
			t.Fatalf("a released index holds no entry, got %+v", st)
		}
	}

	e.EnsureIndex("states", filterB)
	waitReady(t, e, "states", filterB)
	if got, _ := entries(e, "states", filterB); len(got) != 3*3000 {
		t.Fatalf("a released index is rebuilt with its entries, got %d", len(got))
	}

	e.DropIndexes("states")
	snap := e.Snapshot("states")
	for _, spec := range []datastore.IndexSpec{sortA, filterB, refs} {
		if snap.HasIndex(spec) {
			t.Fatalf("%s is dropped", spec.Name())
		}
	}
	if len(e.IndexNames("states")) != 0 || e.IndexBacklog() != 0 {
		t.Fatalf("no index is left after a drop, got %v", e.IndexNames("states"))
	}

	e.EnsureIndex("states", filterB)
	waitReady(t, e, "states", filterB)
	if got, _ := entries(e, "states", filterB); len(got) != 3*3000 {
		t.Fatalf("a dropped index is built again from scratch, got %d", len(got))
	}
}

func TestIndexOnAnEmptyEntityIsReadyAtOnce(t *testing.T) {
	e, err := datastore.Open(options(t.TempDir()))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	spec := datastore.IndexSpec{Kind: datastore.IndexSort, Field: "a"}
	if !e.EnsureIndex("empty", spec) {
		t.Fatalf("an index on an entity without documents is ready at once")
	}

	putDocs(t, e, "empty", 0, 10, workDoc)
	if got, _ := entries(e, "empty", spec); len(got) != 10 {
		t.Fatalf("the writes after the index fill it, got %d entries", len(got))
	}

	if err := e.Update(func(tx *datastore.Tx) error {
		tx.ResetEntity("empty")
		return tx.Put("empty", "again", map[string]any{"id": "again", "a": 1.0})
	}); err != nil {
		t.Fatal(err)
	}

	if got, _ := entries(e, "empty", spec); len(got) != 1 || e.Snapshot("empty").Len() != 1 {
		t.Fatalf("a reset followed by a write in the same transaction keeps only that write, got %d entries", len(got))
	}
}
