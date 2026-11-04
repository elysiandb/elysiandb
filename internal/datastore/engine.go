package datastore

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"sync"
	"sync/atomic"
	"syscall"
	"time"

	"github.com/cockroachdb/pebble/v2"
	"github.com/cockroachdb/pebble/v2/bloom"
	"github.com/taymour/elysiandb/internal/log"
)

const (
	LockFile     = "elysiandb.lock"
	PebbleFolder = "pebble"
	storeFormat  = 3
)

var (
	ErrLocked   = errors.New("the data folder is used by another ElysianDB process")
	ErrClosed   = errors.New("datastore: closed")
	ErrConflict = errors.New("datastore: write conflict")
	engineLog   = log.For("storage")
)

type Durability int

const (
	DurabilityBackground Durability = iota
	DurabilityWrite
	DurabilitySync
)

const (
	defaultCacheBytes    = 256 << 20
	defaultMemTableBytes = 64 << 20
	minMemTableBytes     = 4 << 20
	defaultBuildBytes    = 16 << 20
)

type Options struct {
	Folder         string
	Shards         int
	Durability     Durability
	FlushInterval  time.Duration
	CacheBytes     int64
	MemTableBytes  int64
	BuildBytes     int64
	IndexWorkers   int
	Background     bool
	LegacyRecovery bool
	OnKVExpired    func(n int)
	OnWriteError   func(err error)
	MemoryPressure func() bool
}

var engineInstances atomic.Uint64

type Engine struct {
	instance uint64
	opts     Options
	dir      string
	lock     *os.File
	db       *pebble.DB
	fs       *walFS
	cache    *pebble.Cache

	catalogMu sync.RWMutex
	entities  map[string]*entity
	types     []string
	typesMu   sync.Mutex
	nextID    atomic.Uint32
	lsn       atomic.Uint64
	commits   atomic.Uint64

	kv *kvState

	pinsMu  sync.Mutex
	pins    map[*pin]struct{}
	current *pin

	builds     chan *entity
	buildOrder atomic.Uint64
	compacts   chan [2][]byte
	stop       chan struct{}
	wg         sync.WaitGroup
	closed     atomic.Bool
	writers    sync.WaitGroup
	migrated   *MigrationReport
	unclean    bool
	walBytes   int64

	stats engineStats
}

type engineStats struct {
	conflicts      atomic.Int64
	flushes        atomic.Int64
	flushNanos     atomic.Int64
	writeStalls    atomic.Int64
	stalled        atomic.Bool
	stallStart     atomic.Int64
	stallNanos     atomic.Int64
	builtDocs      atomic.Int64
	backgroundErrs atomic.Int64
}

func Open(opts Options) (*Engine, error) {
	if opts.Shards <= 0 {
		opts.Shards = 64
	}

	if opts.IndexWorkers <= 0 {
		opts.IndexWorkers = 1
	}

	if opts.CacheBytes <= 0 {
		opts.CacheBytes = defaultCacheBytes
	}

	if opts.MemTableBytes <= 0 {
		opts.MemTableBytes = defaultMemTableBytes
	}
	opts.MemTableBytes = max(opts.MemTableBytes, minMemTableBytes)

	if opts.BuildBytes <= 0 {
		opts.BuildBytes = defaultBuildBytes
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
		entities: map[string]*entity{},
		kv:       newKVState(opts.Shards),
		pins:     map[*pin]struct{}{},
		builds:   make(chan *entity, 1024),
		compacts: make(chan [2][]byte, 64),
		stop:     make(chan struct{}),
	}

	start := time.Now()
	report, err := e.migrateIfNeeded()
	if err != nil {
		e.unlock()
		return nil, err
	}
	e.migrated = report

	if err := e.openDB(filepath.Join(opts.Folder, PebbleFolder)); err != nil {
		e.unlock()
		return nil, err
	}

	if err := e.load(); err != nil {
		e.closeDB()
		e.unlock()
		return nil, err
	}

	if opts.Background {
		e.startBackground()
	}

	engineLog.Debug("engine opened", "folder", opts.Folder, "duration_ms", log.Since(start))

	return e, nil
}

const openMark = "open\n"

func lockFolder(folder string) (*os.File, error) {
	f, err := os.OpenFile(filepath.Join(folder, LockFile), os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, err
	}

	if err := syscall.Flock(int(f.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		_ = f.Close()
		return nil, fmt.Errorf("%w (%s)", ErrLocked, folder)
	}

	_ = f.Chmod(0o600)

	return f, nil
}

func (e *Engine) markOpen() (bool, error) {
	buf := make([]byte, len(openMark))
	n, _ := e.lock.ReadAt(buf, 0)
	unclean := string(buf[:n]) == openMark

	if _, err := e.lock.WriteAt([]byte(openMark), 0); err != nil {
		return unclean, err
	}

	return unclean, e.lock.Sync()
}

func (e *Engine) markClosed() {
	if e.lock == nil {
		return
	}

	_ = e.lock.Truncate(0)
	_ = e.lock.Sync()
}

func (e *Engine) unlock() {
	if e.lock != nil {
		_ = syscall.Flock(int(e.lock.Fd()), syscall.LOCK_UN)
		_ = e.lock.Close()
		e.lock = nil
	}
}

type pebbleLogger struct{}

func (pebbleLogger) Infof(format string, args ...any) {
	engineLog.Debug("pebble", "event", fmt.Sprintf(format, args...))
}

func (pebbleLogger) Errorf(format string, args ...any) {
	engineLog.Error("pebble", "err", fmt.Sprintf(format, args...))
}

func (pebbleLogger) Fatalf(format string, args ...any) {
	msg := fmt.Sprintf(format, args...)
	engineLog.Error("storage engine failure", "err", msg)
	panic(msg)
}

func pebbleOptions(e *Engine, cache *pebble.Cache, fs *walFS) *pebble.Options {
	procs := runtime.GOMAXPROCS(0)
	o := &pebble.Options{
		Cache:                       cache,
		FS:                          fs,
		Merger:                      postingsMerger,
		MemTableSize:                uint64(e.opts.MemTableBytes),
		MemTableStopWritesThreshold: 4,
		L0CompactionThreshold:       4,
		L0StopWritesThreshold:       24,
		LBaseMaxBytes:               64 << 20,
		FormatMajorVersion:          pebble.FormatNewest,
		Logger:                      pebbleLogger{},
		CompactionConcurrencyRange: func() (int, int) {
			return 1, max(2, procs/4)
		},
		EventListener: &pebble.EventListener{
			FlushEnd:        e.onFlush,
			BackgroundError: e.onBackgroundError,
			WriteStallBegin: e.onStallBegin,
			WriteStallEnd:   e.onStallEnd,
			DiskSlow: func(info pebble.DiskSlowInfo) {
				engineLog.Warn("slow disk", "path", info.Path, "duration_ms", info.Duration.Milliseconds())
			},
		},
	}

	for i := range o.Levels {
		o.Levels[i].FilterPolicy = bloom.FilterPolicy(10)
		o.Levels[i].FilterType = pebble.TableFilter
	}

	return o
}

func (e *Engine) openDB(dir string) error {
	e.fs = newWALFS(e.opts.Durability != DurabilitySync)
	e.cache = pebble.NewCache(e.opts.CacheBytes)
	e.walBytes = walSize(dir)

	db, err := pebble.Open(dir, pebbleOptions(e, e.cache, e.fs))
	if err != nil {
		e.cache.Unref()
		return fmt.Errorf("cannot open the store in %s: %w", dir, err)
	}
	e.db = db
	ownerOnlyTree(dir)

	return nil
}

func walSize(dir string) int64 {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return 0
	}

	var n int64
	for _, entry := range entries {
		if filepath.Ext(entry.Name()) != ".log" {
			continue
		}
		if info, err := entry.Info(); err == nil {
			n += info.Size()
		}
	}

	return n
}

func (e *Engine) closeDB() error {
	e.closePins()
	err := e.db.Close()
	e.cache.Unref()

	return err
}

func (e *Engine) onFlush(info pebble.FlushInfo) {
	if info.Err != nil || info.Ingest {
		return
	}

	e.stats.flushes.Add(1)
	e.stats.flushNanos.Store(int64(info.TotalDuration))
}

func (e *Engine) onBackgroundError(err error) {
	e.stats.backgroundErrs.Add(1)
	if e.opts.OnWriteError != nil {
		e.opts.OnWriteError(err)
		return
	}

	engineLog.Error("background write failed", "err", err)
}

func (e *Engine) onStallBegin(pebble.WriteStallBeginInfo) {
	e.stats.writeStalls.Add(1)
	e.stats.stalled.Store(true)
	e.stats.stallStart.Store(time.Now().UnixNano())
}

func (e *Engine) onStallEnd() {
	if e.stats.stalled.Swap(false) {
		e.stats.stallNanos.Add(time.Now().UnixNano() - e.stats.stallStart.Load())
	}
}

func (e *Engine) load() error {
	format, closer, err := e.db.Get(metaKey(metaFormat))
	switch {
	case errors.Is(err, pebble.ErrNotFound):
		if err := e.db.Set(metaKey(metaFormat), []byte{storeFormat}, pebble.Sync); err != nil {
			return err
		}
		if err := e.db.Flush(); err != nil {
			return err
		}
	case err != nil:
		return err
	default:
		v := byte(0)
		if len(format) > 0 {
			v = format[0]
		}
		_ = closer.Close()
		if v != storeFormat {
			return fmt.Errorf("the store has format %d, this version reads format %d", v, storeFormat)
		}
	}

	if raw, closer, err := e.db.Get(metaKey(metaTypes)); err == nil {
		types, decodeErr := decodeTypes(raw)
		_ = closer.Close()
		if decodeErr != nil {
			return decodeErr
		}
		e.types = types
	} else if !errors.Is(err, pebble.ErrNotFound) {
		return err
	}

	if err := e.loadEntities(); err != nil {
		return err
	}

	if err := e.kv.load(e.db); err != nil {
		return err
	}

	unclean, err := e.markOpen()
	if err != nil {
		return err
	}

	if unclean {
		e.unclean = true
		engineLog.Warn("unclean shutdown detected, log replayed", "wal_bytes", e.walBytes)
	}

	return nil
}

func (e *Engine) loadEntities() error {
	lower := []byte{familyMeta, metaEntity}
	it, err := e.db.NewIter(&pebble.IterOptions{LowerBound: lower, UpperBound: prefixEnd(lower)})
	if err != nil {
		return err
	}

	var reset *pebble.Batch
	maxID := uint32(0)
	for it.First(); it.Valid(); it.Next() {
		name := string(it.Key()[2:])
		rec, err := decodeRecord(it.Value())
		if err != nil {
			_ = it.Close()
			return fmt.Errorf("entity %q: %w", name, err)
		}

		changed := false
		for _, m := range rec.indexes {
			if m.state != stateBuilding {
				continue
			}

			if reset == nil {
				reset = e.db.NewBatch()
			}
			lo, hi := indexRange(rec.id, m.no)
			_ = reset.DeleteRange(lo, hi, nil)
			m.state = stateDormant
			m.clear()
			changed = true
		}

		if changed {
			_ = reset.Set(entityMetaKey(name), encodeRecord(rec), nil)
		}

		ent := newEntity(name)
		ent.rec.Store(rec)
		e.entities[name] = ent
		maxID = max(maxID, rec.id+1)
		if rec.version > e.lsn.Load() {
			e.lsn.Store(rec.version)
		}
	}

	if err := it.Close(); err != nil {
		return err
	}

	e.nextID.Store(maxID)

	if reset != nil {
		defer reset.Close()
		if err := e.db.Apply(reset, pebble.Sync); err != nil {
			return err
		}
		return e.db.Flush()
	}

	return nil
}

func (e *Engine) startBackground() {
	e.wg.Add(3)
	go e.expiryLoop()
	go e.maintenanceLoop()
	go e.buildLoop()
}

func (e *Engine) Instance() uint64 {
	return e.instance
}

func (e *Engine) Folder() string {
	return e.dir
}

func (e *Engine) UncleanShutdown() bool {
	return e.unclean
}

func (e *Engine) Migration() *MigrationReport {
	return e.migrated
}

func (e *Engine) Close() error {
	if !e.closed.CompareAndSwap(false, true) {
		return nil
	}

	close(e.stop)
	e.wg.Wait()
	e.writers.Wait()

	var errs []error
	if _, err := os.Stat(filepath.Join(e.dir, PebbleFolder)); err != nil {
		e.closeLost()
		e.unlock()
		return fmt.Errorf("the data folder %s is gone: %w", e.dir, err)
	}

	if err := e.db.Flush(); err != nil {
		errs = append(errs, err)
	}

	if err := e.closeDB(); err != nil {
		errs = append(errs, err)
	}

	if len(errs) == 0 {
		e.markClosed()
	}
	e.unlock()

	return errors.Join(errs...)
}

func (e *Engine) closeLost() {
	defer func() {
		if r := recover(); r != nil {
			engineLog.Debug("store closed after its folder was removed", "err", r)
		}
	}()

	_ = e.closeDB()
}

func (e *Engine) Abandon() {
	if !e.closed.CompareAndSwap(false, true) {
		return
	}

	close(e.stop)
	e.wg.Wait()
	e.writers.Wait()
	e.closeLost()
	e.unlock()
}

func (e *Engine) Checkpoint() error {
	if e.closed.Load() {
		return ErrClosed
	}

	if err := probeFolder(e.dir); err != nil {
		return err
	}

	if err := e.fs.syncAll(); err != nil {
		return err
	}

	return e.db.LogData(nil, pebble.Sync)
}

func probeFolder(dir string) error {
	f, err := os.CreateTemp(dir, ".checkpoint-*")
	if err != nil {
		return fmt.Errorf("the data folder is not writable: %w", err)
	}

	name := f.Name()
	_, err = f.Write([]byte{1})

	if closeErr := f.Close(); err == nil {
		err = closeErr
	}

	if removeErr := os.Remove(name); err == nil {
		err = removeErr
	}

	return err
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

func (e *Engine) allEntities() []*entity {
	e.catalogMu.RLock()
	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}
	e.catalogMu.RUnlock()

	sort.Slice(ents, func(i, j int) bool { return ents[i].name < ents[j].name })

	return ents
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

func (e *Engine) EntityVersion(name string) (uint64, uint64) {
	ent := e.entity(name, false)
	if ent == nil {
		return 0, 0
	}

	rec := ent.rec.Load()
	if rec == nil {
		return 0, 0
	}

	return rec.gen, rec.version
}

type applied struct {
	b    *pebble.Batch
	wait bool
	e    *Engine
}

func (e *Engine) apply(b *pebble.Batch, durable bool) (applied, error) {
	var err error
	wait := durable && e.opts.Durability != DurabilityBackground
	if wait {
		err = e.db.ApplyNoSyncWait(b, pebble.Sync)
	} else {
		err = e.db.Apply(b, pebble.NoSync)
	}

	if err != nil {
		_ = b.Close()
		return applied{}, err
	}

	e.commits.Add(1)

	return applied{b: b, wait: wait, e: e}, nil
}

func (a applied) finish() {
	if a.b == nil {
		return
	}

	if a.wait {
		if err := a.b.SyncWait(); err != nil {
			if a.e.opts.OnWriteError != nil {
				a.e.opts.OnWriteError(err)
			} else {
				engineLog.Error("log write failed", "err", err)
			}
		}
	}

	_ = a.b.Close()
}

type pin struct {
	e      *Engine
	ps     *pebble.Snapshot
	refs   atomic.Int64
	at     uint64
	closed atomic.Bool
}

func (e *Engine) acquirePin() *pin {
	e.pinsMu.Lock()
	defer e.pinsMu.Unlock()

	c := e.commits.Load()
	if cur := e.current; cur != nil && cur.at == c && !cur.closed.Load() {
		cur.refs.Add(1)
		return cur
	}

	p := &pin{e: e, ps: e.db.NewSnapshot(), at: c}
	p.refs.Store(2)
	e.pins[p] = struct{}{}
	if old := e.current; old != nil {
		e.releaseLocked(old)
	}
	e.current = p

	return p
}

func (p *pin) release() {
	if p.refs.Add(-1) > 0 {
		return
	}

	p.e.pinsMu.Lock()
	p.e.closePinLocked(p)
	p.e.pinsMu.Unlock()
}

func (e *Engine) releaseLocked(p *pin) {
	if p.refs.Add(-1) == 0 {
		e.closePinLocked(p)
	}
}

func (e *Engine) closePinLocked(p *pin) {
	if p.closed.Swap(true) {
		return
	}

	_ = p.ps.Close()
	delete(e.pins, p)
	if e.current == p {
		e.current = nil
	}
}

func (e *Engine) closePins() {
	e.pinsMu.Lock()
	defer e.pinsMu.Unlock()

	for p := range e.pins {
		e.closePinLocked(p)
	}
	e.current = nil
}

func (p *pin) get(key []byte) ([]byte, bool, error) {
	if p.closed.Load() {
		return nil, false, ErrClosed
	}

	v, closer, err := p.ps.Get(key)
	if errors.Is(err, pebble.ErrNotFound) {
		return nil, false, nil
	}

	if err != nil {
		return nil, false, err
	}

	out := append([]byte(nil), v...)
	_ = closer.Close()

	return out, true, nil
}

func (p *pin) iter(lower, upper []byte) (*pebble.Iterator, error) {
	if p.closed.Load() {
		return nil, ErrClosed
	}

	return p.ps.NewIter(&pebble.IterOptions{LowerBound: lower, UpperBound: upper})
}

func (e *Engine) get(key []byte) ([]byte, bool, error) {
	v, closer, err := e.db.Get(key)
	if errors.Is(err, pebble.ErrNotFound) {
		return nil, false, nil
	}

	if err != nil {
		return nil, false, err
	}

	out := append([]byte(nil), v...)
	_ = closer.Close()

	return out, true, nil
}
