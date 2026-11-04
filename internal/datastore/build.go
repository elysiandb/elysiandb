package datastore

import (
	"encoding/binary"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/log"
)

const (
	smallEntityDocs   = 512
	maxJobIndexes     = 32
	buildFlushDocs    = 4096
	IndexIdleAfter    = time.Hour
	idleSweepInterval = time.Minute
	pressurePause     = 20 * time.Millisecond
)

type usageClock struct {
	at atomic.Int64
}

func (ent *entity) touch(name string) {
	now := time.Now().Unix()
	if v, ok := ent.usage.Load(name); ok {
		last := v.(*usageClock)
		if last.at.Load() != now {
			last.at.Store(now)
		}
		return
	}

	c := &usageClock{}
	c.at.Store(now)
	ent.usage.LoadOrStore(name, c)
}

func (ent *entity) lastUse(name string) time.Time {
	if v, ok := ent.usage.Load(name); ok {
		return time.Unix(v.(*usageClock).at.Load(), 0)
	}

	return time.Time{}
}

type buildJob struct {
	names     map[string]bool
	gen       uint64
	builtUpTo int64
	dirty     *Bitset
	canceled  bool
}

func (j *buildJob) covers(name string) bool {
	return j.names[name]
}

func (e *Engine) writeRecord(ent *entity, rec *entityRecord, ranges [][2][]byte) error {
	b := e.db.NewBatch()
	for _, r := range ranges {
		if err := b.DeleteRange(r[0], r[1], nil); err != nil {
			_ = b.Close()
			return err
		}
	}

	if err := b.Set(entityMetaKey(ent.name), encodeRecord(rec), nil); err != nil {
		_ = b.Close()
		return err
	}

	a, err := e.apply(b, false)
	if err != nil {
		return err
	}
	if err := a.finish(); err != nil {
		return err
	}
	ent.rec.Store(rec)

	return nil
}

func (e *Engine) EnsureIndex(name string, spec IndexSpec) bool {
	key := spec.Name()
	ent := e.entity(name, true)
	if m := ent.rec.Load().index(key); m != nil && m.state != stateDormant {
		if m.state == stateReady {
			ent.touch(key)
		}
		return m.state == stateReady
	}

	if !e.enter() {
		return false
	}
	defer e.leave()

	ent.mu.Lock()
	rec := ent.rec.Load()
	if m := rec.index(key); m != nil && m.state != stateDormant {
		ent.mu.Unlock()
		return m.state == stateReady
	}

	ent.touch(key)
	var next *entityRecord
	if rec == nil {
		next = &entityRecord{id: e.nextID.Add(1) - 1}
	} else {
		next = rec.clone()
	}

	m := next.index(key)
	if m == nil {
		m = next.addIndex(spec, stateBuilding)
	}
	m.state = stateBuilding
	m.clear()
	if next.count == 0 {
		m.state = stateReady
	}

	err := e.writeRecord(ent, next, nil)
	ready := m.state == stateReady
	small := next.count <= smallEntityDocs
	ent.mu.Unlock()

	if err != nil {
		engineLog.Error("cannot register an index", "entity", name, "index", key, "err", err)
		return false
	}

	if ready {
		return true
	}

	if !small && e.schedule(ent) {
		return false
	}

	e.buildEntity(ent, false)

	m = ent.rec.Load().index(key)

	return m != nil && m.state == stateReady
}

func (e *Engine) schedule(ent *entity) bool {
	if !e.opts.Background || e.closed.Load() {
		return false
	}

	if !ent.queued.CompareAndSwap(false, true) {
		return true
	}

	select {
	case e.builds <- ent:
		return true
	default:
		ent.queued.Store(false)
		return false
	}
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
		case ent := <-e.builds:
			sem <- struct{}{}
			running.Add(1)
			go func() {
				defer func() {
					<-sem
					running.Done()
				}()
				e.buildEntity(ent, true)
			}()
		}
	}
}

func (e *Engine) buildEntity(ent *entity, background bool) {
	if background {
		ent.queued.Store(false)
	}

	for {
		if !e.buildPass(ent, background) {
			return
		}

		if background && e.schedule(ent) {
			return
		}
	}
}

func (e *Engine) claim(ent *entity) (*buildJob, *pin, *entityRecord, []*indexMeta) {
	ent.mu.Lock()
	defer ent.mu.Unlock()

	rec := ent.rec.Load()
	if rec == nil || ent.job != nil || e.closed.Load() {
		return nil, nil, nil, nil
	}

	var pending []*indexMeta
	for _, m := range rec.indexes {
		if m.state == stateBuilding {
			pending = append(pending, m)
		}
	}

	if len(pending) == 0 {
		return nil, nil, nil, nil
	}

	sort.Slice(pending, func(i, j int) bool { return pending[i].no < pending[j].no })
	if len(pending) > maxJobIndexes {
		pending = pending[:maxJobIndexes]
	}

	next := rec.clone()
	ranges := make([][2][]byte, 0, len(pending))
	job := &buildJob{names: map[string]bool{}, gen: rec.gen, builtUpTo: -1, dirty: NewBitset(rec.nextSeq)}
	for i, m := range pending {
		job.names[m.name] = true
		lo, hi := indexRange(next.id, m.no)
		ranges = append(ranges, [2][]byte{lo, hi})
		pending[i] = next.index(m.name)
		pending[i].clear()
	}

	if err := e.writeRecord(ent, next, ranges); err != nil {
		engineLog.Error("cannot start an index build", "entity", ent.name, "err", err)
		return nil, nil, nil, nil
	}
	ent.job = job

	p := e.acquirePin()
	if p == nil {
		ent.job = nil
		return nil, nil, nil, nil
	}

	return job, p, next, pending
}

type builtDoc struct {
	seq   uint32
	keys  [][]string
	grams []uint32
}

func (e *Engine) underPressure() bool {
	return e.opts.MemoryPressure != nil && e.opts.MemoryPressure()
}

func (e *Engine) stopping() bool {
	select {
	case <-e.stop:
		return true
	default:
		return false
	}
}

func (e *Engine) buildPass(ent *entity, background bool) bool {
	job, p, rec, metas := e.claim(ent)
	if job == nil {
		return false
	}
	defer p.release()

	start := time.Now()
	pending := []builtDoc{}
	size := 0
	docs := 0
	aborted := false

	flush := func(final bool) bool {
		if !e.flushBuild(ent, job, metas, pending, final) {
			aborted = true
			return false
		}
		pending = pending[:0]
		size = 0
		return true
	}

	lower := partPrefix(rec.id, partDocs)
	it, err := p.iter(lower, prefixEnd(lower))
	if err != nil {
		e.abandonJob(ent, job)
		return false
	}

	for it.First(); it.Valid(); it.Next() {
		key := it.Key()
		if len(key) != 10 {
			continue
		}

		for background && e.underPressure() && !e.stopping() {
			time.Sleep(pressurePause)
		}

		if e.stopping() {
			aborted = true
			break
		}

		_, _, value, ok := decodeDocValue(it.Value())
		if !ok {
			continue
		}

		doc := decodeDoc(value)
		bd := builtDoc{seq: binary.BigEndian.Uint32(key[6:]), keys: make([][]string, len(metas))}
		for i, m := range metas {
			if m.spec.Kind == IndexSearch {
				bd.grams = searchGrams(doc)
				size += 2 * len(bd.grams)
				continue
			}

			bd.keys[i] = uniqueKeys(extractKeys(m.spec, doc))
			for _, k := range bd.keys[i] {
				size += len(k) + 24
			}
		}

		pending = append(pending, bd)
		docs++
		if (len(pending) >= buildFlushDocs || int64(size) >= e.opts.BuildBytes) && !flush(false) {
			break
		}
	}

	_ = it.Close()

	if aborted {
		e.abandonJob(ent, job)
		return false
	}

	if !flush(true) {
		e.abandonJob(ent, job)
		return false
	}

	e.stats.builtDocs.Add(int64(docs))
	engineLog.Debug("indexes built", "entity", ent.name, "indexes", len(metas), "documents", docs, "duration_ms", log.Since(start))

	return e.hasPendingBuilds(ent)
}

func (e *Engine) abandonJob(ent *entity, job *buildJob) {
	ent.mu.Lock()
	defer ent.mu.Unlock()

	if ent.job != job {
		return
	}
	ent.job = nil

	rec := ent.rec.Load()
	if rec == nil || rec.gen != job.gen || e.closed.Load() {
		return
	}

	next := rec.clone()
	ranges := [][2][]byte{}
	for name := range job.names {
		m := next.index(name)
		if m == nil || m.state != stateBuilding {
			continue
		}
		m.state = stateDormant
		m.clear()
		lo, hi := indexRange(next.id, m.no)
		ranges = append(ranges, [2][]byte{lo, hi})
	}

	if len(ranges) == 0 {
		return
	}

	if err := e.writeRecord(ent, next, ranges); err != nil {
		engineLog.Error("cannot release an interrupted index build", "entity", ent.name, "err", err)
	}
}

func (e *Engine) hasPendingBuilds(ent *entity) bool {
	ent.mu.Lock()
	defer ent.mu.Unlock()

	if ent.job != nil {
		return false
	}

	for _, m := range ent.rec.Load().indexes {
		if m.state == stateBuilding {
			return true
		}
	}

	return false
}

func (e *Engine) flushBuild(ent *entity, job *buildJob, metas []*indexMeta, pending []builtDoc, final bool) bool {
	ent.mu.Lock()
	defer ent.mu.Unlock()

	cur := ent.rec.Load()
	if job.canceled || ent.job != job || cur == nil || cur.gen != job.gen || e.closed.Load() {
		return false
	}

	next := cur.clone()
	b := e.db.NewBatch()
	postings := map[string][]uint16{}
	var sk [4]byte
	for _, bd := range pending {
		if job.dirty.Has(bd.seq) {
			continue
		}

		binary.BigEndian.PutUint32(sk[:], bd.seq)
		for i, meta := range metas {
			m := next.index(meta.name)
			if m == nil || m.state != stateBuilding {
				continue
			}

			if m.spec.Kind == IndexSearch {
				if len(bd.grams) == 0 {
					continue
				}
				m.entries++
				chunk := bd.seq >> postingChunkBits
				off := uint16(bd.seq & (postingChunk - 1))
				for _, g := range bd.grams {
					k := string(indexKey(next.id, m.no, postingEntry(g, chunk)))
					postings[k] = append(postings[k], off)
				}
				continue
			}

			counted := m.counted()
			for _, k := range bd.keys[i] {
				if err := b.Set(indexKey(next.id, m.no, append([]byte(k), sk[:]...)), nil, nil); err != nil {
					_ = b.Close()
					return false
				}
				m.entries++
				if counted {
					m.addKind(k[0], 1)
				}
			}
		}
	}

	for k, offs := range postings {
		var adds, removes chunkBits
		for _, off := range offs {
			adds.set(off)
		}
		if err := b.Merge([]byte(k), encodeDelta(&adds, &removes), nil); err != nil {
			_ = b.Close()
			return false
		}
	}

	if len(pending) > 0 {
		job.builtUpTo = int64(pending[len(pending)-1].seq)
	}

	if final {
		for _, meta := range metas {
			if m := next.index(meta.name); m != nil && m.state == stateBuilding {
				m.state = stateReady
			}
		}
	}

	if err := b.Set(entityMetaKey(ent.name), encodeRecord(next), nil); err != nil {
		_ = b.Close()
		return false
	}

	a, err := e.apply(b, false)
	if err != nil {
		engineLog.Error("index build write failed", "entity", ent.name, "err", err)
		return false
	}
	if err := a.finish(); err != nil {
		return false
	}

	ent.rec.Store(next)
	if final {
		ent.job = nil
	}

	return true
}

func (e *Engine) DropIndexes(name string) {
	ent := e.entity(name, false)
	if ent == nil || !e.enter() {
		return
	}
	defer e.leave()

	ent.mu.Lock()
	defer ent.mu.Unlock()

	rec := ent.rec.Load()
	if rec == nil || len(rec.indexes) == 0 {
		return
	}

	next := rec.clone()
	ranges := [][2][]byte{}
	for _, m := range next.indexes {
		lo, hi := indexRange(next.id, m.no)
		ranges = append(ranges, [2][]byte{lo, hi})
	}
	next.indexes = nil

	if ent.job != nil {
		ent.job.canceled = true
		ent.job = nil
	}

	if err := e.writeRecord(ent, next, ranges); err != nil {
		engineLog.Error("cannot drop indexes", "entity", name, "err", err)
	}
}

func (e *Engine) IndexNames(name string) []string {
	ent := e.entity(name, false)
	if ent == nil {
		return nil
	}

	rec := ent.rec.Load()
	if rec == nil {
		return []string{}
	}

	names := make([]string, 0, len(rec.indexes))
	for _, m := range rec.indexes {
		names = append(names, m.name)
	}

	return names
}

func (e *Engine) IndexBacklog() int {
	n := 0
	for _, ent := range e.allEntities() {
		for _, m := range ent.rec.Load().indexesOrNil() {
			if m.state == stateBuilding {
				n++
			}
		}
	}

	return n
}

func (r *entityRecord) indexesOrNil() []*indexMeta {
	if r == nil {
		return nil
	}

	return r.indexes
}

func (e *Engine) DropIdleIndexes(idle time.Duration) int {
	if !e.enter() {
		return 0
	}
	defer e.leave()

	cutoff := time.Now().Add(-idle)
	dropped := 0
	for _, ent := range e.allEntities() {
		ent.mu.Lock()
		rec := ent.rec.Load()
		var next *entityRecord
		ranges := [][2][]byte{}
		for _, m := range rec.indexesOrNil() {
			if m.state != stateReady {
				continue
			}

			last := ent.lastUse(m.name)
			if last.IsZero() {
				ent.touch(m.name)
				continue
			}

			if !last.Before(cutoff) {
				continue
			}

			if next == nil {
				next = rec.clone()
			}

			nm := next.index(m.name)
			nm.state = stateDormant
			nm.clear()
			lo, hi := indexRange(next.id, nm.no)
			ranges = append(ranges, [2][]byte{lo, hi})
			dropped++
		}

		if next != nil {
			if err := e.writeRecord(ent, next, ranges); err != nil {
				engineLog.Error("cannot release idle indexes", "entity", ent.name, "err", err)
			}
		}
		ent.mu.Unlock()
	}

	if dropped > 0 {
		engineLog.Debug("idle indexes released", "indexes", dropped, "idle_minutes", int(idle.Minutes()))
	}

	return dropped
}

type IndexState struct {
	Entity   string
	Index    string
	State    string
	Entries  int
	LastUsed time.Time
}

func (e *Engine) IndexStates() []IndexState {
	out := []IndexState{}
	for _, ent := range e.allEntities() {
		for _, m := range ent.rec.Load().indexesOrNil() {
			st := IndexState{Entity: ent.name, Index: m.name, LastUsed: ent.lastUse(m.name)}
			if m.spec.Kind == IndexSearch {
				st.Index = "search"
			}

			switch m.state {
			case stateDormant:
				st.State = "released"
			case stateReady:
				st.State = "ready"
				st.Entries = int(m.entries)
			default:
				st.State = "building"
			}
			out = append(out, st)
		}
	}

	sort.Slice(out, func(i, j int) bool {
		if out[i].Entity != out[j].Entity {
			return out[i].Entity < out[j].Entity
		}
		return out[i].Index < out[j].Index
	})

	return out
}

func (v *IndexView) postings(gram uint32) *Bitset {
	defer runtime.KeepAlive(v.s)

	out := &Bitset{}
	lower := concat(v.prefix, []byte{byte(gram >> 16), byte(gram >> 8), byte(gram)})
	it, err := v.s.p.iter(lower, prefixEnd(lower))
	if err != nil {
		return out
	}
	defer it.Close()

	n := len(v.prefix)
	for it.First(); it.Valid(); it.Next() {
		k := it.Key()
		if len(k) != n+7 {
			continue
		}
		out.addContainer(binary.BigEndian.Uint32(k[n+3:]), it.Value())
	}

	return out
}

func (e *Engine) SearchCandidates(name string, pattern string) (*Bitset, bool) {
	patternGrams := PatternTrigrams(pattern)
	if len(patternGrams) == 0 {
		return nil, false
	}

	grams := make([]uint32, len(patternGrams))
	for i, g := range patternGrams {
		grams[i] = gramOf(g, 0)
	}
	sort.Slice(grams, func(i, j int) bool { return grams[i] < grams[j] })

	if e.entity(name, false) == nil {
		return &Bitset{}, true
	}

	snap := e.Snapshot(name)
	view, ok := snap.Index(searchSpec())
	if !ok {
		if snap.Len() > 0 && (!snap.HasIndex(searchSpec()) || snap.IndexDormant(searchSpec())) {
			e.EnsureIndex(name, searchSpec())
			snap = e.Snapshot(name)
			view, ok = snap.Index(searchSpec())
		}
		if !ok {
			return nil, false
		}
	}

	var out *Bitset
	for _, g := range grams {
		set := view.postings(g)
		if out == nil {
			out = set
		} else {
			out.And(set)
		}
		if out.Count() == 0 {
			return out, true
		}
	}

	return out, true
}

func (e *Engine) scheduleCompaction(r [2][]byte) {
	if !e.opts.Background {
		return
	}

	select {
	case e.compacts <- r:
	default:
	}
}

func (e *Engine) compactRange(r [2][]byte) {
	if _, err := os.Stat(filepath.Join(e.dir, PebbleFolder)); err != nil {
		return
	}

	start := time.Now()
	if err := e.db.Compact(e.ctx, r[0], r[1], false); err != nil {
		if !e.closed.Load() {
			engineLog.Debug("range compaction failed", "err", err)
		}
		return
	}

	engineLog.Debug("deleted range compacted", "duration_ms", log.Since(start))
}

func (e *Engine) maintenanceLoop() {
	defer e.wg.Done()

	idle := time.NewTicker(idleSweepInterval)
	defer idle.Stop()

	var sync <-chan time.Time
	if e.opts.FlushInterval > 0 {
		t := time.NewTicker(e.opts.FlushInterval)
		defer t.Stop()
		sync = t.C
	}

	for {
		select {
		case <-e.stop:
			return
		case <-idle.C:
			e.DropIdleIndexes(IndexIdleAfter)
		case <-sync:
			if err := e.syncLog(); err != nil && e.opts.OnWriteError != nil {
				e.opts.OnWriteError(err)
			}
		case r := <-e.compacts:
			e.compactRange(r)
		}
	}
}
