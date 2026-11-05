package datastore

import (
	"errors"
	"fmt"
	"github.com/taymour/elysiandb/internal/durable"
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
)

const (
	LockFile     = "elysiandb.lock"
	LogFolder    = "log"
	ManifestFile = "MANIFEST"
)

var (
	ErrLocked      = errors.New("the data folder is used by another ElysianDB process")
	ErrClosed      = errors.New("datastore: closed")
	ErrConflict    = errors.New("datastore: write conflict")
	ErrDiskFull    = errors.New("disk full")
	ErrWriteFailed = errors.New("storage write failed")
	engineLog      = log.For("storage")
)

type Options struct {
	Folder          string
	Shards          int
	Durability      journal.Mode
	FlushInterval   time.Duration
	CheckpointEvery time.Duration
	MaxLogBytes     int64
	CacheBytes      int64
	PageCacheBytes  int64
	PageSegmentSize int64
	DirtyFlushBytes int64
	IndexWorkers    int
	Background      bool
	SegmentSize     int64
	FS              journal.FS
	LegacyRecovery  bool
	OnKVExpired     func(n int)
	OnWriteError    func(err error)
	MinFreeBytes    int64
	FreeSpace       func() (int64, error)
}

var engineInstances atomic.Uint64

type Engine struct {
	instance  uint64
	opts      Options
	dir       string
	lock      *os.File
	j         *journal.Journal
	inline    bool
	cache     *valueCache
	pages     *durable.PageStore
	manifests *durable.Manifests
	current   *manifest
	previous  *manifest
	live      liveBytes

	catalogMu sync.RWMutex
	entities  map[string]*entity
	types     []string
	typesMu   sync.Mutex

	kv *kvStore

	ckptMu          sync.Mutex
	ckState         atomic.Pointer[checkpointState]
	checkpointBytes atomic.Int64
	segmentLive     atomic.Pointer[map[uint32]int64]

	dirty          atomic.Int64
	flushNow       chan struct{}
	flushed        chan struct{}
	flushedMu      sync.Mutex
	builds         chan *entity
	buildOrder     atomic.Uint64
	upgradeFrom    int
	pageRemoval    []uint32
	lastPageWalk   time.Time
	lastReclaim    time.Time
	reclaim        atomic.Int32
	diskLow        atomic.Bool
	lastDiskCheck  time.Time
	pressureSweeps int
	stop           chan struct{}
	wg             sync.WaitGroup
	closeMu        sync.Mutex
	closed         atomic.Bool
	writers        sync.WaitGroup
	migrated       *MigrationReport

	stats engineStats
}

type engineStats struct {
	checkpoints       atomic.Int64
	checkpointNanos   atomic.Int64
	compactedSegments atomic.Int64
	compactedBytes    atomic.Int64
	conflicts         atomic.Int64
	replayedRecords   atomic.Int64
	livePages         atomic.Int64
	refusedWrites     atomic.Int64
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
		instance:  engineInstances.Add(1),
		opts:      opts,
		dir:       opts.Folder,
		lock:      lock,
		inline:    opts.CacheBytes <= 0,
		cache:     newValueCache(valueCacheLimit(opts.CacheBytes), 64),
		entities:  map[string]*entity{},
		kv:        newKVStore(opts.Shards),
		manifests: durable.OpenManifests(opts.FS, opts.Folder),
		builds:    make(chan *entity, 1024),
		flushNow:  make(chan struct{}, 1),
		flushed:   make(chan struct{}),
		stop:      make(chan struct{}),
	}

	start := time.Now()
	report, err := e.migrateIfNeeded()
	if err != nil {
		e.unlock()
		return nil, err
	}
	e.migrated = report

	pages, err := durable.OpenPageStore(opts.FS, filepath.Join(opts.Folder, durable.PagesFolder), opts.PageCacheBytes, opts.PageSegmentSize)
	if err != nil {
		e.unlock()
		return nil, err
	}
	e.pages = pages
	if opts.Background {
		pages.Pace(e.flushPause)
	}

	if err := e.load(); err != nil {
		e.pages.Close()
		e.unlock()
		return nil, err
	}

	if e.upgradeFrom != 0 {
		if err := e.finishUpgrade(); err != nil {
			e.j.Abandon()
			e.pages.Close()
			e.unlock()
			return nil, err
		}
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
	o := journal.Options{
		Mode:          e.opts.Durability,
		FlushInterval: e.opts.FlushInterval,
		SegmentSize:   e.opts.SegmentSize,
		FS:            e.opts.FS,
		OnWriteError:  e.opts.OnWriteError,
	}
	if e.opts.Background {
		o.SyncBytes = logSyncBytes
	}

	return o
}

func (e *Engine) load() error {
	j, err := journal.Open(filepath.Join(e.dir, LogFolder), e.journalOptions())
	if err != nil {
		return err
	}
	e.j = j

	m, previous, err := e.chooseManifest()
	if err != nil {
		return err
	}
	e.current, e.previous = m, previous

	from := journal.Position{Segment: 1}
	switch {
	case m != nil && m.Format == manifestFormatPages:
		if err := e.restorePages(m); err != nil {
			return err
		}

		from = journal.Position{Segment: m.Segment, Offset: m.Offset}
		e.ckState.Store(&checkpointState{pos: from, lsn: m.LSN})
		e.j.SetNextLSN(m.LSN + 1)
	case m != nil:
		ck, err := readCheckpoint(filepath.Join(e.dir, m.Checkpoint))
		if err != nil {
			return fmt.Errorf("cannot read the checkpoint %s: %w", m.Checkpoint, err)
		}

		if err := e.restore(ck); err != nil {
			return err
		}

		from = ck.pos
		e.upgradeFrom = m.Format
		e.ckState.Store(&checkpointState{pos: ck.pos, lsn: ck.lastLSN})
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

	if len(stats.SetAside) > 0 || len(stats.Damaged) > 0 {
		engineLog.Error("the log is damaged before its end, what follows the damage is set aside and not replayed", "segments", stats.SetAside, "damaged_tails", stats.Damaged, "suffix", ".damaged")
	}

	if stats.Records > 0 {
		engineLog.Warn("unclean shutdown detected, log replayed", "batches", stats.Batches, "records", stats.Records)
	}

	e.j.Start()

	return nil
}

func (e *Engine) startBackground() {
	e.wg.Add(4)
	go e.expiryLoop()
	go e.maintenanceLoop()
	go e.buildLoop()
	go e.dirtyLoop()

	if e.inline {
		go e.warmValues()
	}
}

func (e *Engine) RunBackground(fn func(stop <-chan struct{})) {
	if !e.opts.Background {
		return
	}

	e.closeMu.Lock()
	defer e.closeMu.Unlock()

	if e.closed.Load() {
		return
	}

	e.wg.Add(1)
	go func() {
		defer e.wg.Done()
		fn(e.stop)
	}()
}

func (e *Engine) Close() error {
	e.closeMu.Lock()
	if !e.closed.CompareAndSwap(false, true) {
		e.closeMu.Unlock()
		return nil
	}
	e.closeMu.Unlock()

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

	if err := e.pages.Close(); err != nil {
		errs = append(errs, err)
	}

	e.unlock()

	return errors.Join(errs...)
}

func (e *Engine) Abandon() {
	e.closeMu.Lock()
	if !e.closed.CompareAndSwap(false, true) {
		e.closeMu.Unlock()
		return
	}
	e.closeMu.Unlock()

	close(e.stop)
	e.wg.Wait()
	e.j.Abandon()
	e.pages.Close()
	e.unlock()
}

const (
	diskCheckEvery  = time.Second
	diskRecoverGain = 4
)

func (e *Engine) checkDisk() {
	if e.opts.MinFreeBytes <= 0 || time.Since(e.lastDiskCheck) < diskCheckEvery {
		return
	}
	e.lastDiskCheck = time.Now()

	read := e.opts.FreeSpace
	if read == nil {
		read = func() (int64, error) { return durable.FreeBytes(e.dir) }
	}

	free, err := read()
	if err != nil {
		return
	}

	threshold := e.opts.MinFreeBytes
	switch {
	case free < threshold && e.diskLow.CompareAndSwap(false, true):
		engineLog.Error("disk almost full, writes are refused until space comes back", "free_bytes", free, "min_free_bytes", threshold)
	case free >= threshold+threshold/diskRecoverGain && e.diskLow.CompareAndSwap(true, false):
		engineLog.Info("disk space is back, writes are accepted again", "free_bytes", free, "min_free_bytes", threshold)
	}
}

func (e *Engine) WriteRefusal() error {
	if err := e.j.Failure(); err != nil {
		if errors.Is(err, syscall.ENOSPC) {
			return ErrDiskFull
		}
		return ErrWriteFailed
	}

	if e.diskLow.Load() {
		return ErrDiskFull
	}

	return nil
}

func (e *Engine) NoteRefusedWrite() {
	e.stats.refusedWrites.Add(1)
}

func (e *Engine) FlushLog() error {
	return e.j.Flush()
}

func (e *Engine) RefusedWrites() int64 {
	return e.stats.refusedWrites.Load()
}

func (e *Engine) refuse(err error) error {
	e.stats.refusedWrites.Add(1)

	return err
}

func (e *Engine) guarded(task string, fn func()) {
	defer func() {
		r := recover()
		if r == nil {
			return
		}

		if suppressed, ok := writeFailures.Allow(task); ok {
			engineLog.Error("a background task failed, it will run again", log.WithSuppressed([]any{"task", task, "err", r}, suppressed)...)
		}
	}()

	fn()
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
		ent = e.newEntity(name)
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
		if released := drafts[name].released; len(released) > 0 {
			e.releaseValues(released)
		}
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
		return &Snapshot{e: e, name: name, r: e.emptyRoot(0, 0)}
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

func (s *Snapshot) Instance() uint64 {
	return s.e.instance
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
	slot slot
	lazy bool
}

func (d Doc) resolved() slot {
	if !d.lazy {
		return d.slot
	}

	sl, _ := d.s.r.slotBySeq(d.slot.seq, true)

	return sl
}

func (d Doc) ID() string {
	return d.resolved().id
}

func (d Doc) Seq() uint32 {
	return d.slot.seq
}

func (d Doc) LSN() uint64 {
	return d.resolved().lsn
}

func (d Doc) Bytes() ([]byte, error) {
	return d.s.e.slotBytes(d.resolved())
}

func (d Doc) Decode() (map[string]any, error) {
	sl := d.resolved()
	b, err := d.s.e.slotBytes(sl)
	if err != nil {
		return nil, err
	}

	doc := decodeDoc(b)
	if doc == nil {
		return nil, fmt.Errorf("document %s is not a JSON object", sl.id)
	}

	return doc, nil
}

func (s *Snapshot) Indexed(seq uint32) Doc {
	return Doc{s: s, slot: slot{seq: seq}, lazy: true}
}

func (s *Snapshot) Get(id string) (Doc, bool) {
	sl, ok := s.r.slotByID(id, true)
	if !ok {
		return Doc{}, false
	}

	return Doc{s: s, slot: sl}, true
}

func (s *Snapshot) BySeq(seq uint32) (Doc, bool) {
	sl, ok := s.r.slotBySeq(seq, true)
	if !ok {
		return Doc{}, false
	}

	return Doc{s: s, slot: sl}, true
}

func (s *Snapshot) At(i int) (Doc, bool) {
	k, v, ok := s.r.slots.At(i)
	if !ok {
		return Doc{}, false
	}

	return Doc{s: s, slot: decodeSlotValue(keySeq(k), v, true)}, true
}

func (s *Snapshot) Ascend(offset int, fn func(Doc) bool) {
	s.r.slots.AscendAt(offset, func(k string, v []byte) bool {
		return fn(Doc{s: s, slot: decodeSlotValue(keySeq(k), v, true)})
	})
}

const (
	scanWindowBytes = 4 << 20
	logSyncBytes    = 8 << 20
)

func (s *Snapshot) ScanValues(fn func(d Doc, raw []byte, err error) bool) {
	s.ScanValuesPaced(nil, fn)
}

func (s *Snapshot) ScanValuesPaced(pace func(time.Duration), fn func(d Doc, raw []byte, err error) bool) {
	window := journal.NewReadWindow(scanWindowBytes, pace)
	s.r.slots.Scan(func(k string, v []byte) bool {
		sl := decodeSlotValue(keySeq(k), v, true)
		d := Doc{s: s, slot: sl}
		if s.e.closed.Load() {
			fn(d, nil, ErrClosed)
			return false
		}

		if b, ok := s.e.cache.peek(sl.lsn); ok {
			return fn(d, b, nil)
		}

		raw, err := s.e.j.ReadThrough(sl.loc, window)

		return fn(d, raw, err)
	})
}

func (s *Snapshot) RankOfSeq(seq uint32) int {
	return s.r.slots.Rank(seqKey(seq))
}

type IndexView struct {
	s      *Snapshot
	idx    *index
	prefix string
	base   int
	end    int
	sub    bool
}

func (s *Snapshot) Index(spec IndexSpec) (*IndexView, bool) {
	name := spec.Name()
	idx, ok := s.r.indexes[name]
	if !ok || !idx.ready {
		return nil, false
	}

	if s.ent != nil {
		s.ent.touch(name)
	}

	return &IndexView{s: s, idx: idx}, true
}

func (s *Snapshot) HasIndex(spec IndexSpec) bool {
	_, ok := s.r.indexes[spec.Name()]
	return ok
}

func (s *Snapshot) IndexDormant(spec IndexSpec) bool {
	idx, ok := s.r.indexes[spec.Name()]
	return ok && idx.dormant
}

func (v *IndexView) Len() int {
	if v.sub {
		return v.subLen()
	}

	if v.bitmap() {
		return v.bitmapLen()
	}

	return v.idx.tree.Len()
}

func (v *IndexView) Rank(prefix []byte) int {
	if v.sub {
		return v.subRank(prefix)
	}

	if v.bitmap() {
		return v.bitmapRank(prefix)
	}

	return v.idx.tree.Rank(string(prefix))
}

func (v *IndexView) ScanPrefix(prefix []byte, fn func(key string, seq uint32) bool) {
	if v.bitmap() {
		v.bitmapScan(prefix, prefixEnd(prefix), fn)
		return
	}

	p := string(prefix)
	v.idx.tree.AscendFrom(p, func(k string, _ []byte) bool {
		if len(k) < len(p)+4 || k[:len(p)] != p {
			return false
		}

		return fn(k, keySeq(k))
	})
}

func (v *IndexView) ScanRange(lo, hi []byte, fn func(key string, seq uint32) bool) {
	if v.bitmap() {
		v.bitmapScan(lo, hi, fn)
		return
	}

	h := string(hi)
	v.idx.tree.AscendFrom(string(lo), func(k string, _ []byte) bool {
		if hi != nil && k >= h {
			return false
		}

		return fn(k, keySeq(k))
	})
}

func prefixEnd(prefix []byte) []byte {
	end := append([]byte(nil), prefix...)
	for i := len(end) - 1; i >= 0; i-- {
		if end[i] < 0xff {
			end[i]++
			return end[:i+1]
		}
	}

	return nil
}

func (v *IndexView) AscendAt(i int, fn func(key string, seq uint32) bool) {
	if v.sub {
		v.subAscendAt(i, fn)
		return
	}

	v.idx.tree.AscendAt(i, func(k string, _ []byte) bool {
		return fn(k, keySeq(k))
	})
}

func (v *IndexView) At(i int) (string, uint32, bool) {
	if v.sub {
		return v.subAt(i)
	}

	k, _, ok := v.idx.tree.At(i)
	if !ok {
		return "", 0, false
	}

	return k, keySeq(k), true
}

func (e *Engine) EnsureIndex(name string, spec IndexSpec) bool {
	key := spec.Name()
	ent := e.entity(name, true)
	r := ent.root.Load()
	if idx, ok := r.indexes[key]; ok && !idx.dormant {
		return idx.ready
	}

	ent.apply.Lock()
	r = ent.root.Load()
	if idx, ok := r.indexes[key]; ok && !idx.dormant {
		ent.apply.Unlock()
		return idx.ready
	}

	ent.touch(key)
	nr := *r
	nr.indexes = cloneIndexes(r.indexes)
	if r.slots.Len() == 0 {
		nr.indexes[key] = &index{spec: spec, ready: true}
		ent.root.Store(&nr)
		ent.apply.Unlock()
		return true
	}

	nr.indexes[key] = &index{spec: spec}
	ent.building[key] = &buildState{order: e.buildOrder.Add(1)}
	ent.root.Store(&nr)
	small := r.slots.Len() <= smallEntityDocs
	ent.apply.Unlock()

	if !small && e.schedule(ent) {
		return false
	}

	e.buildEntity(ent, false)

	idx, ok := ent.root.Load().indexes[key]

	return ok && idx.ready
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
			if !idx.ready && !idx.dormant {
				n++
			}
		}
	}

	return n
}
