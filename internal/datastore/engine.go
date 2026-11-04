package datastore

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"sync"
	"sync/atomic"
	"syscall"
	"time"

	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/ptree"
)

const (
	LockFile     = "elysiandb.lock"
	LogFolder    = "log"
	ManifestFile = "MANIFEST"
)

var (
	ErrLocked   = errors.New("the data folder is used by another ElysianDB process")
	ErrClosed   = errors.New("datastore: closed")
	ErrConflict = errors.New("datastore: write conflict")
	engineLog   = log.For("storage")
)

type Options struct {
	Folder          string
	Shards          int
	Durability      journal.Mode
	FlushInterval   time.Duration
	CheckpointEvery time.Duration
	MaxLogBytes     int64
	CacheBytes      int64
	IndexWorkers    int
	Background      bool
	SegmentSize     int64
	FS              journal.FS
	LegacyRecovery  bool
	OnKVExpired     func(n int)
	OnWriteError    func(err error)
}

var engineInstances atomic.Uint64

type Engine struct {
	instance uint64
	opts     Options
	dir      string
	lock     *os.File
	j        *journal.Journal
	inline   bool
	cache    *valueCache

	catalogMu sync.RWMutex
	entities  map[string]*entity
	types     []string
	typesMu   sync.Mutex

	kv *kvStore

	ckptMu          sync.Mutex
	checkpointPos   journal.Position
	checkpointLSN   uint64
	checkpointAt    time.Time
	checkpointBytes atomic.Int64
	segmentLive     atomic.Pointer[map[uint32]int64]

	builds   chan buildJob
	stop     chan struct{}
	wg       sync.WaitGroup
	closed   atomic.Bool
	writers  sync.WaitGroup
	migrated *MigrationReport

	stats engineStats
}

type engineStats struct {
	checkpoints       atomic.Int64
	checkpointNanos   atomic.Int64
	compactedSegments atomic.Int64
	compactedBytes    atomic.Int64
	conflicts         atomic.Int64
	replayedRecords   atomic.Int64
}

func Open(opts Options) (*Engine, error) {
	if opts.Shards <= 0 {
		opts.Shards = 64
	}

	if opts.IndexWorkers <= 0 {
		opts.IndexWorkers = 1
	}

	if err := os.MkdirAll(opts.Folder, 0o755); err != nil {
		return nil, err
	}

	lock, err := lockFolder(opts.Folder)
	if err != nil {
		return nil, err
	}

	e := &Engine{
		instance: engineInstances.Add(1),
		opts:     opts,
		dir:      opts.Folder,
		lock:     lock,
		inline:   opts.CacheBytes <= 0,
		cache:    newValueCache(max(opts.CacheBytes, 1), 64),
		entities: map[string]*entity{},
		kv:       newKVStore(opts.Shards),
		builds:   make(chan buildJob, 1024),
		stop:     make(chan struct{}),
	}

	start := time.Now()
	report, err := e.migrateIfNeeded()
	if err != nil {
		e.unlock()
		return nil, err
	}
	e.migrated = report

	if err := e.load(); err != nil {
		e.unlock()
		return nil, err
	}

	if opts.Background {
		e.startBackground()
	}

	engineLog.Debug("engine opened", "folder", opts.Folder, "duration_ms", log.Since(start))

	return e, nil
}

func lockFolder(folder string) (*os.File, error) {
	f, err := os.OpenFile(filepath.Join(folder, LockFile), os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, err
	}

	if err := syscall.Flock(int(f.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		_ = f.Close()
		return nil, fmt.Errorf("%w (%s)", ErrLocked, folder)
	}

	return f, nil
}

func (e *Engine) unlock() {
	if e.lock != nil {
		_ = syscall.Flock(int(e.lock.Fd()), syscall.LOCK_UN)
		_ = e.lock.Close()
		e.lock = nil
	}
}

func (e *Engine) Instance() uint64 {
	return e.instance
}

func (e *Engine) Folder() string {
	return e.dir
}

func (e *Engine) ReplayedRecords() int64 {
	return e.stats.replayedRecords.Load()
}

func (e *Engine) Migration() *MigrationReport {
	return e.migrated
}

func (e *Engine) journalOptions() journal.Options {
	return journal.Options{
		Mode:          e.opts.Durability,
		FlushInterval: e.opts.FlushInterval,
		SegmentSize:   e.opts.SegmentSize,
		FS:            e.opts.FS,
		OnWriteError:  e.opts.OnWriteError,
	}
}

func (e *Engine) load() error {
	j, err := journal.Open(filepath.Join(e.dir, LogFolder), e.journalOptions())
	if err != nil {
		return err
	}
	e.j = j

	m, err := readManifest(e.dir)
	if err != nil {
		return err
	}

	from := journal.Position{Segment: 1}
	if m != nil {
		ck, err := readCheckpoint(filepath.Join(e.dir, m.Checkpoint))
		if err != nil {
			return fmt.Errorf("cannot read the checkpoint %s: %w", m.Checkpoint, err)
		}

		if err := e.restore(ck); err != nil {
			return err
		}

		from = ck.pos
		e.checkpointPos = ck.pos
		e.checkpointLSN = ck.lastLSN
		e.j.SetNextLSN(ck.lastLSN + 1)
	}

	stats, err := e.j.Replay(from, func(recs []journal.Record) error {
		e.applyBatch(recs, nil, true)
		return nil
	})
	if err != nil {
		return fmt.Errorf("cannot replay the log: %w", err)
	}

	e.stats.replayedRecords.Store(int64(stats.Records))
	if stats.Truncated > 0 {
		engineLog.Warn("torn batch dropped at the end of the log", "bytes", stats.Truncated)
	}

	if stats.Records > 0 {
		engineLog.Warn("unclean shutdown detected, log replayed", "batches", stats.Batches, "records", stats.Records)
	}

	e.j.Start()

	return nil
}

func (e *Engine) startBackground() {
	e.wg.Add(3)
	go e.expiryLoop()
	go e.maintenanceLoop()
	go e.buildLoop()
}

func (e *Engine) Close() error {
	if !e.closed.CompareAndSwap(false, true) {
		return nil
	}

	close(e.stop)
	e.wg.Wait()
	e.writers.Wait()

	var errs []error
	if err := e.checkpoint(true); err != nil {
		errs = append(errs, err)
	}

	if err := e.j.Close(); err != nil {
		errs = append(errs, err)
	}

	e.unlock()

	return errors.Join(errs...)
}

func (e *Engine) Abandon() {
	if !e.closed.CompareAndSwap(false, true) {
		return
	}

	close(e.stop)
	e.wg.Wait()
	e.j.Abandon()
	e.unlock()
}

func (e *Engine) entity(name string, create bool) *entity {
	e.catalogMu.RLock()
	ent := e.entities[name]
	e.catalogMu.RUnlock()
	if ent != nil || !create {
		return ent
	}

	e.catalogMu.Lock()
	defer e.catalogMu.Unlock()

	if ent = e.entities[name]; ent == nil {
		ent = newEntity(name)
		e.entities[name] = ent
	}

	return ent
}

func (e *Engine) Types() []string {
	e.catalogMu.RLock()
	defer e.catalogMu.RUnlock()

	return append([]string(nil), e.types...)
}

func (e *Engine) TypeExists(name string) bool {
	e.catalogMu.RLock()
	defer e.catalogMu.RUnlock()

	return slices.Contains(e.types, name)
}

func (e *Engine) addTypeLocked(name string) {
	if !slices.Contains(e.types, name) {
		e.types = append(append([]string(nil), e.types...), name)
	}
}

func (e *Engine) dropTypeLocked(name string) {
	kept := make([]string, 0, len(e.types))
	for _, t := range e.types {
		if t != name {
			kept = append(kept, t)
		}
	}
	e.types = kept
}

func (e *Engine) applyBatch(recs []journal.Record, docs []map[string]any, replay bool) {
	drafts := map[string]*draft{}
	order := []string{}
	get := func(name string) *draft {
		if d := drafts[name]; d != nil {
			return d
		}

		d := e.newDraft(e.entity(name, true))
		drafts[name] = d
		order = append(order, name)

		return d
	}

	for i := range recs {
		rec := &recs[i]
		var doc map[string]any
		if docs != nil {
			doc = docs[i]
		}

		switch rec.Kind {
		case journal.KindDocPut, journal.KindDocMove:
			get(rec.Entity).put(rec, doc, replay)
		case journal.KindDocDelete:
			get(rec.Entity).del(rec, replay)
		case journal.KindEntityReset:
			get(rec.Entity).reset(rec)
		case journal.KindEntityCreate:
			e.catalogMu.Lock()
			e.addTypeLocked(rec.Entity)
			e.catalogMu.Unlock()
		case journal.KindEntityDrop:
			e.catalogMu.Lock()
			e.dropTypeLocked(rec.Entity)
			e.catalogMu.Unlock()
		case journal.KindKVPut, journal.KindKVMove:
			e.kv.applyPut(rec, replay)
		case journal.KindKVDelete, journal.KindKVExpire:
			e.kv.applyDelete(rec.Key)
		case journal.KindKVReset:
			e.kv.clearLocked(rec.LSN)
		}
	}

	for _, name := range order {
		drafts[name].publish()
	}
}

type Snapshot struct {
	e    *Engine
	ent  *entity
	r    *root
	name string
}

func (e *Engine) Snapshot(name string) *Snapshot {
	ent := e.entity(name, false)
	if ent == nil {
		return &Snapshot{e: e, name: name, r: &root{indexes: map[string]*index{}}}
	}

	return &Snapshot{e: e, ent: ent, r: ent.root.Load(), name: name}
}

func (e *Engine) SnapshotAll() map[string]*Snapshot {
	e.catalogMu.RLock()
	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}
	e.catalogMu.RUnlock()

	sort.Slice(ents, func(i, j int) bool { return ents[i].name < ents[j].name })
	for _, ent := range ents {
		ent.apply.Lock()
	}

	out := make(map[string]*Snapshot, len(ents))
	for _, ent := range ents {
		out[ent.name] = &Snapshot{e: e, ent: ent, r: ent.root.Load(), name: ent.name}
	}

	for i := len(ents) - 1; i >= 0; i-- {
		ents[i].apply.Unlock()
	}

	return out
}

func (s *Snapshot) Name() string {
	return s.name
}

func (s *Snapshot) Len() int {
	return s.r.slots.Len()
}

func (s *Snapshot) Version() uint64 {
	return s.r.version
}

func (s *Snapshot) Generation() uint64 {
	return s.r.gen
}

func (s *Snapshot) NextSeq() uint32 {
	return s.r.nextSeq
}

func (s *Snapshot) Refs() []RefPath {
	return s.r.refs
}

type Doc struct {
	s    *Snapshot
	slot *slot
}

func (d Doc) ID() string {
	return d.slot.id
}

func (d Doc) Seq() uint32 {
	return d.slot.seq
}

func (d Doc) LSN() uint64 {
	return d.slot.lsn
}

func (d Doc) Bytes() ([]byte, error) {
	return d.s.e.slotBytes(d.slot)
}

func (d Doc) Decode() (map[string]any, error) {
	b, err := d.Bytes()
	if err != nil {
		return nil, err
	}

	doc := decodeDoc(b)
	if doc == nil {
		return nil, fmt.Errorf("document %s is not a JSON object", d.slot.id)
	}

	return doc, nil
}

func (s *Snapshot) Get(id string) (Doc, bool) {
	sl, ok := s.r.ids.Get(id)
	if !ok {
		return Doc{}, false
	}

	return Doc{s: s, slot: sl}, true
}

func (s *Snapshot) BySeq(seq uint32) (Doc, bool) {
	sl, ok := s.r.slots.Get(seqKey(seq))
	if !ok {
		return Doc{}, false
	}

	return Doc{s: s, slot: sl}, true
}

func (s *Snapshot) At(i int) (Doc, bool) {
	_, sl, ok := s.r.slots.At(i)
	if !ok {
		return Doc{}, false
	}

	return Doc{s: s, slot: sl}, true
}

func (s *Snapshot) Ascend(offset int, fn func(Doc) bool) {
	s.r.slots.AscendAt(offset, func(_ string, sl *slot) bool {
		return fn(Doc{s: s, slot: sl})
	})
}

func (s *Snapshot) RankOfSeq(seq uint32) int {
	return s.r.slots.Rank(seqKey(seq))
}

type IndexView struct {
	s   *Snapshot
	idx *index
}

func (s *Snapshot) Index(spec IndexSpec) (*IndexView, bool) {
	idx, ok := s.r.indexes[spec.Name()]
	if !ok || !idx.ready {
		return nil, false
	}

	return &IndexView{s: s, idx: idx}, true
}

func (s *Snapshot) HasIndex(spec IndexSpec) bool {
	_, ok := s.r.indexes[spec.Name()]
	return ok
}

func (v *IndexView) Len() int {
	return v.idx.tree.Len()
}

func (v *IndexView) Rank(prefix []byte) int {
	return v.idx.tree.Rank(string(prefix))
}

func (v *IndexView) ScanPrefix(prefix []byte, fn func(key string, seq uint32) bool) {
	p := string(prefix)
	v.idx.tree.AscendFrom(p, func(k string, _ struct{}) bool {
		if len(k) < len(p)+4 || k[:len(p)] != p {
			return false
		}

		return fn(k, keySeq(k))
	})
}

func (v *IndexView) ScanRange(lo, hi []byte, fn func(key string, seq uint32) bool) {
	h := string(hi)
	v.idx.tree.AscendFrom(string(lo), func(k string, _ struct{}) bool {
		if hi != nil && k >= h {
			return false
		}

		return fn(k, keySeq(k))
	})
}

func (v *IndexView) AscendAt(i int, fn func(key string, seq uint32) bool) {
	v.idx.tree.AscendAt(i, func(k string, _ struct{}) bool {
		return fn(k, keySeq(k))
	})
}

func (v *IndexView) At(i int) (string, uint32, bool) {
	k, _, ok := v.idx.tree.At(i)
	if !ok {
		return "", 0, false
	}

	return k, keySeq(k), true
}

func (e *Engine) EnsureIndex(name string, spec IndexSpec) bool {
	ent := e.entity(name, true)
	r := ent.root.Load()
	if idx, ok := r.indexes[spec.Name()]; ok {
		return idx.ready
	}

	ent.apply.Lock()
	r = ent.root.Load()
	if idx, ok := r.indexes[spec.Name()]; ok {
		ent.apply.Unlock()
		return idx.ready
	}

	if r.slots.Len() == 0 {
		nr := *r
		nr.indexes = cloneIndexes(r.indexes)
		nr.indexes[spec.Name()] = &index{spec: spec, ready: true}
		ent.root.Store(&nr)
		ent.apply.Unlock()
		return true
	}

	nr := *r
	nr.indexes = cloneIndexes(r.indexes)
	nr.indexes[spec.Name()] = &index{spec: spec}
	ent.building[spec.Name()] = &buildState{dirty: map[uint32]struct{}{}}
	ent.root.Store(&nr)
	ent.apply.Unlock()

	job := buildJob{entity: ent, spec: spec}
	if e.opts.Background && !e.closed.Load() {
		select {
		case e.builds <- job:
			return false
		default:
		}
	}

	e.build(job)

	return true
}

func cloneIndexes(m map[string]*index) map[string]*index {
	out := make(map[string]*index, len(m)+1)
	for k, v := range m {
		out[k] = v
	}

	return out
}

func (e *Engine) DropIndexes(name string) {
	ent := e.entity(name, false)
	if ent == nil {
		return
	}

	ent.apply.Lock()
	r := *ent.root.Load()
	r.indexes = map[string]*index{}
	ent.building = map[string]*buildState{}
	ent.root.Store(&r)
	ent.search.Store(nil)
	ent.apply.Unlock()
}

func (e *Engine) IndexNames(name string) []string {
	ent := e.entity(name, false)
	if ent == nil {
		return nil
	}

	names := []string{}
	for n := range ent.root.Load().indexes {
		names = append(names, n)
	}
	sort.Strings(names)

	return names
}

func (e *Engine) IndexBacklog() int {
	e.catalogMu.RLock()
	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}
	e.catalogMu.RUnlock()

	n := 0
	for _, ent := range ents {
		for _, idx := range ent.root.Load().indexes {
			if !idx.ready {
				n++
			}
		}
	}

	return n
}

type buildJob struct {
	entity *entity
	spec   IndexSpec
}

func (e *Engine) buildLoop() {
	defer e.wg.Done()

	sem := make(chan struct{}, e.opts.IndexWorkers)
	var running sync.WaitGroup
	for {
		select {
		case <-e.stop:
			running.Wait()
			return
		case job := <-e.builds:
			sem <- struct{}{}
			running.Add(1)
			go func() {
				defer func() {
					<-sem
					running.Done()
				}()
				e.build(job)
			}()
		}
	}
}

func (e *Engine) build(job buildJob) {
	ent := job.entity
	name := job.spec.Name()
	start := time.Now()
	snap := ent.root.Load()

	keys := make([]string, 0, snap.slots.Len())
	snapKeys := map[uint32][]string{}
	snap.slots.AscendAt(0, func(_ string, s *slot) bool {
		doc := e.decodeSlot(s)
		ks := uniqueKeys(extractKeys(job.spec, doc))
		snapKeys[s.seq] = ks
		for _, k := range ks {
			keys = append(keys, k+seqKey(s.seq))
		}
		return true
	})

	sort.Strings(keys)
	vals := make([]struct{}, len(keys))
	tree := ptree.FromSorted(keys, vals)

	ent.apply.Lock()
	defer ent.apply.Unlock()

	cur := ent.root.Load()
	st := ent.building[name]
	idx, still := cur.indexes[name]
	if st == nil || !still || idx.ready {
		return
	}

	if cur.gen != snap.gen {
		delete(ent.building, name)
		nr := *cur
		nr.indexes = cloneIndexes(cur.indexes)
		delete(nr.indexes, name)
		ent.root.Store(&nr)
		return
	}

	for seq := range st.dirty {
		for _, k := range snapKeys[seq] {
			tree, _ = tree.Delete(k + seqKey(seq))
		}

		if s, ok := cur.slots.Get(seqKey(seq)); ok {
			for _, k := range uniqueKeys(extractKeys(job.spec, e.decodeSlot(s))) {
				tree, _ = tree.Put(k+seqKey(seq), struct{}{})
			}
		}
	}

	delete(ent.building, name)
	nr := *cur
	nr.indexes = cloneIndexes(cur.indexes)
	nr.indexes[name] = &index{spec: job.spec, ready: true, tree: tree}
	ent.root.Store(&nr)

	engineLog.Debug("index built", "entity", ent.name, "index", name, "entries", tree.Len(), "duration_ms", log.Since(start))
}
