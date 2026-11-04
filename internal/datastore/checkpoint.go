package datastore

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/taymour/elysiandb/internal/durable"
	"hash/crc32"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/pagetree"
)

const (
	checkpointMagic     = "ELYCKPT1"
	checkpointVersion   = 1
	manifestFormat      = 1
	manifestFormatPages = 2
	LegacyCheckpoints   = "legacy-checkpoint"
	pageWalkEvery       = time.Minute
	pageVictimsPerCycle = 2
)

var (
	castagnoli    = crc32.MakeTable(crc32.Castagnoli)
	errCheckpoint = errors.New("checkpoint is corrupt")
)

type manifest struct {
	Format     int              `json:"format"`
	Checkpoint string           `json:"checkpoint,omitempty"`
	KV         string           `json:"kv,omitempty"`
	LSN        uint64           `json:"lsn"`
	Segment    uint32           `json:"segment"`
	Offset     uint32           `json:"offset"`
	CreatedAt  string           `json:"created_at"`
	Version    string           `json:"elysiandb_version,omitempty"`
	Types      []string         `json:"types,omitempty"`
	Entities   []manifestEntity `json:"entities,omitempty"`
	Live       map[string]int64 `json:"live,omitempty"`
}

type pageRoot struct {
	ID   uint64 `json:"id"`
	Size uint32 `json:"size"`
}

type manifestIndex struct {
	Name    string   `json:"name"`
	Root    pageRoot `json:"root"`
	Dormant bool     `json:"dormant,omitempty"`
}

type manifestEntity struct {
	Name    string            `json:"name"`
	Gen     uint64            `json:"gen"`
	NextSeq uint32            `json:"next_seq"`
	Version uint64            `json:"version"`
	Refs    []RefPath         `json:"refs,omitempty"`
	Slots   pageRoot          `json:"slots"`
	IDs     pageRoot          `json:"ids"`
	Indexes []manifestIndex   `json:"indexes,omitempty"`
	Derived []manifestIndex   `json:"derived_indexes,omitempty"`
	Origins map[string]string `json:"index_origins,omitempty"`
}

func (me *manifestEntity) addIndex(spec IndexSpec, mi manifestIndex) {
	if spec.Derived() {
		me.Derived = append(me.Derived, mi)
		return
	}

	me.Indexes = append(me.Indexes, mi)
}

func parseManifest(data []byte) (*manifest, error) {
	var m manifest
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("cannot read %s: %w", ManifestFile, err)
	}

	if m.Format != manifestFormat && m.Format != manifestFormatPages {
		return nil, fmt.Errorf("%s has format %d, this version reads formats %d and %d", ManifestFile, m.Format, manifestFormat, manifestFormatPages)
	}

	return &m, nil
}

func (e *Engine) files() durable.FS {
	return durable.Or(e.opts.FS)
}

func (m *manifest) roots() []pageRoot {
	var out []pageRoot
	for _, me := range m.Entities {
		out = append(out, me.Slots, me.IDs)
		for _, mi := range append(append([]manifestIndex(nil), me.Indexes...), me.Derived...) {
			if !mi.Dormant {
				out = append(out, mi.Root)
			}
		}
	}

	return out
}

func (e *Engine) checkManifest(m *manifest) error {
	if m.Format != manifestFormatPages {
		return nil
	}

	if m.KV != "" {
		if _, err := durable.ReadKVFile(e.files(), filepath.Join(e.dir, m.KV)); err != nil {
			return fmt.Errorf("cannot read %s: %w", m.KV, err)
		}
	}

	for _, r := range m.roots() {
		if r.ID == 0 {
			continue
		}
		if _, err := e.pages.Load(pagetree.StoredRef(r.ID, r.Size)); err != nil {
			return err
		}
	}

	return nil
}

func (e *Engine) chooseManifest() (*manifest, *manifest, error) {
	stored, err := e.manifests.Stored()
	if err != nil {
		return nil, nil, err
	}

	var chosen, previous *manifest
	var reasons []string
	for i, sm := range stored {
		m, err := parseManifest(sm.Data)
		if err == nil && chosen == nil {
			err = e.checkManifest(m)
		}

		if err != nil {
			reasons = append(reasons, fmt.Sprintf("%s: %v", sm.Name, err))
			continue
		}

		if chosen == nil {
			chosen = m
			e.manifests.Adopt(sm.Data)
			if i > 0 {
				engineLog.Warn("the last checkpoint is unreadable, the previous one is used", "reason", reasons[0])
			}
			continue
		}

		previous = m
	}

	if chosen == nil && len(stored) > 0 {
		return nil, nil, fmt.Errorf("no readable checkpoint: %s", strings.Join(reasons, "; "))
	}

	return chosen, previous, nil
}

func rootOf(t pagetree.Tree) pageRoot {
	if r := t.Root(); r != nil {
		return pageRoot{ID: r.ID(), Size: r.Size()}
	}

	return pageRoot{}
}

func (e *Engine) openTree(r pageRoot) pagetree.Tree {
	if r.ID == 0 {
		return pagetree.New(e.pages)
	}

	return pagetree.Open(e.pages, pagetree.StoredRef(r.ID, r.Size))
}

type ckReader struct {
	data []byte
	pos  int
	err  error
}

func (c *ckReader) uvarint() uint64 {
	if c.err != nil {
		return 0
	}

	v, n := binary.Uvarint(c.data[c.pos:])
	if n <= 0 {
		c.err = errCheckpoint
		return 0
	}

	c.pos += n

	return v
}

func (c *ckReader) varint() int64 {
	if c.err != nil {
		return 0
	}

	v, n := binary.Varint(c.data[c.pos:])
	if n <= 0 {
		c.err = errCheckpoint
		return 0
	}

	c.pos += n

	return v
}

func (c *ckReader) str() string {
	l := c.uvarint()
	if c.err != nil {
		return ""
	}

	if uint64(len(c.data)-c.pos) < l {
		c.err = errCheckpoint
		return ""
	}

	s := string(c.data[c.pos : c.pos+int(l)])
	c.pos += int(l)

	return s
}

func (c *ckReader) loc() journal.Location {
	return journal.Location{Segment: uint32(c.uvarint()), Offset: uint32(c.uvarint()), Length: uint32(c.uvarint())}
}

type ckEntity struct {
	name    string
	gen     uint64
	nextSeq uint32
	version uint64
	refs    []RefPath
	slots   []*slot
	indexes []ckIndex
}

type ckExtras struct {
	dormant []IndexSpec
	search  bool
}

const ckExtrasSection = 1

type ckIndex struct {
	spec IndexSpec
	keys []string
}

type ckKV struct {
	key   string
	entry *kvEntry
}

type checkpointData struct {
	lastLSN  uint64
	pos      journal.Position
	types    []string
	entities []ckEntity
	kv       []ckKV
	extras   map[string]ckExtras
}

func readCheckpoint(path string) (*checkpointData, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	if len(data) < len(checkpointMagic)+4 || string(data[:len(checkpointMagic)]) != checkpointMagic {
		return nil, errCheckpoint
	}

	body := data[:len(data)-4]
	if crc32.Checksum(body, castagnoli) != binary.LittleEndian.Uint32(data[len(data)-4:]) {
		return nil, errCheckpoint
	}

	r := &ckReader{data: body, pos: len(checkpointMagic)}
	if v := r.uvarint(); v != checkpointVersion {
		return nil, fmt.Errorf("checkpoint version %d is not supported", v)
	}

	ck := &checkpointData{}
	ck.lastLSN = r.uvarint()
	ck.pos = journal.Position{Segment: uint32(r.uvarint()), Offset: uint32(r.uvarint())}

	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		ck.types = append(ck.types, r.str())
	}

	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		ent := ckEntity{name: r.str(), gen: r.uvarint(), nextSeq: uint32(r.uvarint()), version: r.uvarint()}
		for m := r.uvarint(); m > 0 && r.err == nil; m-- {
			ent.refs = append(ent.refs, RefPath{Path: r.str(), Target: r.str()})
		}

		count := r.uvarint()
		ent.slots = make([]*slot, 0, min(count, 1<<20))
		for m := count; m > 0 && r.err == nil; m-- {
			ent.slots = append(ent.slots, &slot{seq: uint32(r.uvarint()), id: r.str(), lsn: r.uvarint(), loc: r.loc()})
		}

		for m := r.uvarint(); m > 0 && r.err == nil; m-- {
			spec, ok := ParseIndexName(r.str())
			if !ok && r.err == nil {
				r.err = errCheckpoint
			}

			idx := ckIndex{spec: spec}
			entries := r.uvarint()
			idx.keys = make([]string, 0, min(entries, 1<<20))
			for k := entries; k > 0 && r.err == nil; k-- {
				idx.keys = append(idx.keys, r.str())
			}
			ent.indexes = append(ent.indexes, idx)
		}

		ck.entities = append(ck.entities, ent)
	}

	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		key := r.str()
		ck.kv = append(ck.kv, ckKV{key: key, entry: &kvEntry{lsn: r.uvarint(), expiresAt: r.varint(), loc: r.loc()}})
	}

	if r.err == nil && r.pos < len(r.data) && r.uvarint() == ckExtrasSection {
		ck.extras = map[string]ckExtras{}
		for n := r.uvarint(); n > 0 && r.err == nil; n-- {
			name := r.str()
			var x ckExtras
			for m := r.uvarint(); m > 0 && r.err == nil; m-- {
				if spec, ok := ParseIndexName(r.str()); ok {
					x.dormant = append(x.dormant, spec)
				}
			}
			x.search = r.uvarint() == 1
			ck.extras[name] = x
		}
	}

	if r.err != nil {
		return nil, r.err
	}

	return ck, nil
}

type segmentCache struct {
	j    *journal.Journal
	fs   durable.FS
	id   uint32
	data []byte
}

func (c *segmentCache) value(loc journal.Location) ([]byte, error) {
	if loc.Length == 0 {
		return []byte{}, nil
	}

	if c.data == nil || c.id != loc.Segment {
		data, err := durable.ReadFile(c.fs, filepath.Join(c.j.Dir(), journal.SegmentName(loc.Segment)))
		if err != nil {
			return nil, err
		}

		c.id, c.data = loc.Segment, data
	}

	end := int(loc.Offset) + int(loc.Length)
	if end > len(c.data) {
		return nil, fmt.Errorf("segment %d is shorter than a value it holds", loc.Segment)
	}

	return append([]byte(nil), c.data[loc.Offset:end]...), nil
}

func (e *Engine) restore(ck *checkpointData) error {
	e.catalogMu.Lock()
	e.types = append([]string(nil), ck.types...)
	e.catalogMu.Unlock()

	live := map[uint32]int64{}
	for _, ce := range ck.entities {
		ent := e.entity(ce.name, true)

		bySeq := append([]*slot(nil), ce.slots...)
		sort.Slice(bySeq, func(i, j int) bool { return bySeq[i].seq < bySeq[j].seq })
		seqKeys := make([]string, len(bySeq))
		seqVals := make([][]byte, len(bySeq))
		for i, sl := range bySeq {
			seqKeys[i] = seqKey(sl.seq)
			seqVals[i] = encodeSlot(nil, *sl)
			live[sl.loc.Segment] += int64(sl.loc.Length)
		}

		byID := append([]*slot(nil), ce.slots...)
		sort.Slice(byID, func(i, j int) bool { return byID[i].id < byID[j].id })
		idKeys := make([]string, len(byID))
		idVals := make([][]byte, len(byID))
		for i, sl := range byID {
			idKeys[i] = sl.id
			idVals[i] = seqBytes(sl.seq)
		}

		r := &root{
			gen:     ce.gen,
			nextSeq: ce.nextSeq,
			version: ce.version,
			refs:    ce.refs,
			slots:   pagetree.FromSorted(e.pages, seqKeys, seqVals),
			ids:     pagetree.FromSorted(e.pages, idKeys, idVals),
			indexes: map[string]*index{},
		}

		for _, ci := range ce.indexes {
			r.indexes[ci.spec.Name()] = &index{spec: ci.spec, ready: true, tree: pagetree.FromSorted(e.pages, ci.keys, nil)}
		}

		if x, ok := ck.extras[ce.name]; ok {
			for _, spec := range x.dormant {
				if _, exists := r.indexes[spec.Name()]; !exists {
					r.indexes[spec.Name()] = &index{spec: spec, dormant: true}
				}
			}
		}

		ent.root.Store(r)
	}

	e.live.reset(live)

	return e.restoreKV(ck.kv)
}

func (e *Engine) restoreKV(entries []ckKV) error {
	kvLoads := append([]ckKV(nil), entries...)
	sort.Slice(kvLoads, func(i, j int) bool {
		a, b := kvLoads[i].entry.loc, kvLoads[j].entry.loc
		if a.Segment != b.Segment {
			return a.Segment < b.Segment
		}
		return a.Offset < b.Offset
	})

	sc := &segmentCache{j: e.j, fs: e.files()}
	for _, kv := range kvLoads {
		v, err := sc.value(kv.entry.loc)
		if err != nil {
			return err
		}

		kv.entry.value = v
		sh := e.kv.shard(kv.key)
		sh.mu.Lock()
		e.kv.setLocked(sh, kv.key, kv.entry)
		sh.mu.Unlock()
	}

	return nil
}

func (e *Engine) restorePages(m *manifest) error {
	e.catalogMu.Lock()
	e.types = append([]string(nil), m.Types...)
	e.catalogMu.Unlock()

	for _, me := range m.Entities {
		ent := e.entity(me.Name, true)
		r := &root{
			gen:     me.Gen,
			nextSeq: me.NextSeq,
			version: me.Version,
			refs:    me.Refs,
			slots:   e.openTree(me.Slots),
			ids:     e.openTree(me.IDs),
			indexes: map[string]*index{},
		}

		for name, origin := range me.Origins {
			ent.meta(name).setOrigin(IndexOrigin(origin))
		}

		for _, mi := range append(append([]manifestIndex(nil), me.Indexes...), me.Derived...) {
			spec, ok := ParseIndexName(mi.Name)
			if !ok {
				return fmt.Errorf("%s names an unknown index %q", ManifestFile, mi.Name)
			}

			if mi.Dormant {
				r.indexes[mi.Name] = &index{spec: spec, dormant: true}
				continue
			}

			r.indexes[mi.Name] = &index{spec: spec, ready: true, tree: e.openTree(mi.Root)}
		}

		ent.root.Store(r)
	}

	live := map[uint32]int64{}
	for seg, n := range m.Live {
		id, err := strconv.ParseUint(seg, 10, 32)
		if err != nil {
			return fmt.Errorf("%s has an invalid segment %q", ManifestFile, seg)
		}
		live[uint32(id)] = n
	}
	e.live.reset(live)

	if m.KV == "" {
		return nil
	}

	records, err := durable.ReadKVFile(e.files(), filepath.Join(e.dir, m.KV))
	if err != nil {
		return fmt.Errorf("cannot read the checkpoint %s: %w", m.KV, err)
	}

	kv := make([]ckKV, 0, len(records))
	for _, r := range records {
		kv = append(kv, ckKV{key: r.Key, entry: &kvEntry{lsn: r.LSN, expiresAt: r.ExpiresAt, loc: journal.Location{Segment: r.Segment, Offset: r.Offset, Length: r.Length}}})
	}

	return e.restoreKV(kv)
}

func (e *Engine) finishUpgrade() error {
	if err := e.checkpoint(true); err != nil {
		return fmt.Errorf("cannot write the upgraded checkpoint: %w", err)
	}

	files := e.files()
	entries, err := files.ReadDir(e.dir)
	if err != nil {
		return err
	}

	legacy := filepath.Join(e.dir, LegacyCheckpoints)
	moved := 0
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasPrefix(name, "checkpoint-") || !strings.HasSuffix(name, ".ckpt") {
			continue
		}

		if err := files.MkdirAll(legacy, 0o755); err != nil {
			return err
		}

		if err := files.Rename(filepath.Join(e.dir, name), filepath.Join(legacy, name)); err != nil {
			return err
		}
		moved++
	}

	_ = files.SyncDir(e.dir)
	engineLog.Info("storage format upgraded", "from", e.upgradeFrom, "to", manifestFormatPages, "kept_in", LegacyCheckpoints, "files", moved)
	e.upgradeFrom = 0

	return nil
}

type checkpointState struct {
	pos journal.Position
	lsn uint64
	at  time.Time
}

func (e *Engine) lastCheckpoint() checkpointState {
	if st := e.ckState.Load(); st != nil {
		return *st
	}

	return checkpointState{}
}

type entitySnapshot struct {
	ent    *entity
	name   string
	r      *root
	search bool
}

func (e *Engine) Checkpoint() error {
	return e.checkpoint(false)
}

func (e *Engine) checkpoint(force bool) error {
	e.ckptMu.Lock()
	defer e.ckptMu.Unlock()

	start := time.Now()
	pos := e.j.Position()
	lsn := e.j.NextLSN() - 1
	if last := e.lastCheckpoint(); !force && pos == last.pos && !last.at.IsZero() {
		return nil
	}

	e.catalogMu.RLock()
	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}
	e.catalogMu.RUnlock()
	sort.Slice(ents, func(i, j int) bool { return ents[i].name < ents[j].name })

	snaps := make([]entitySnapshot, 0, len(ents))
	for _, ent := range ents {
		ent.apply.Lock()
		snaps = append(snaps, entitySnapshot{ent: ent, name: ent.name, r: ent.root.Load()})
		ent.apply.Unlock()
	}

	e.typesMu.Lock()
	types := e.Types()
	e.typesMu.Unlock()

	kv := []ckKV{}
	for _, sh := range e.kv.shards {
		sh.mu.RLock()
		for k, entry := range sh.m {
			kv = append(kv, ckKV{key: k, entry: entry})
		}
		sh.mu.RUnlock()
	}
	sort.Slice(kv, func(i, j int) bool { return kv[i].key < kv[j].key })

	docLive := e.live.snapshot()

	if err := e.j.Flush(); err != nil {
		return fmt.Errorf("cannot flush the log before a checkpoint: %w", err)
	}

	m := manifest{Format: manifestFormatPages, LSN: lsn, Segment: pos.Segment, Offset: pos.Offset, Types: types, Live: map[string]int64{}}
	for _, snap := range snaps {
		trees := []pagetree.Tree{snap.r.slots, snap.r.ids}
		me := manifestEntity{Name: snap.name, Gen: snap.r.gen, NextSeq: snap.r.nextSeq, Version: snap.r.version, Refs: snap.r.refs}

		names := make([]string, 0, len(snap.r.indexes))
		for name := range snap.r.indexes {
			names = append(names, name)
		}
		sort.Strings(names)

		for _, name := range names {
			idx := snap.r.indexes[name]
			if origin := snap.ent.meta(name).Origin(); origin != OriginQuery {
				if me.Origins == nil {
					me.Origins = map[string]string{}
				}
				me.Origins[name] = string(origin)
			}
			switch {
			case idx.dormant:
				me.addIndex(idx.spec, manifestIndex{Name: name, Dormant: true})
			case idx.ready:
				trees = append(trees, idx.tree)
			}
		}

		for _, t := range trees {
			if err := e.pages.FlushTree(t); err != nil {
				return fmt.Errorf("cannot write the pages of %s: %w", snap.name, err)
			}
		}

		me.Slots, me.IDs = rootOf(snap.r.slots), rootOf(snap.r.ids)
		for _, name := range names {
			if idx := snap.r.indexes[name]; idx.ready {
				me.addIndex(idx.spec, manifestIndex{Name: name, Root: rootOf(idx.tree)})
			}
		}
		sort.Slice(me.Indexes, func(i, j int) bool { return me.Indexes[i].Name < me.Indexes[j].Name })
		sort.Slice(me.Derived, func(i, j int) bool { return me.Derived[i].Name < me.Derived[j].Name })

		m.Entities = append(m.Entities, me)
	}

	if err := e.pages.Sync(); err != nil {
		return fmt.Errorf("cannot sync the pages: %w", err)
	}

	live := map[uint32]int64{}
	for seg, n := range docLive {
		live[seg] += n
	}

	m.KV = fmt.Sprintf("kv-%016d.ckpt", lsn)
	records := make([]durable.KVRecord, 0, len(kv))
	for _, item := range kv {
		loc := item.entry.loc
		records = append(records, durable.KVRecord{Key: item.key, LSN: item.entry.lsn, ExpiresAt: item.entry.expiresAt, Segment: loc.Segment, Offset: loc.Offset, Length: loc.Length})
		live[loc.Segment] += int64(loc.Length)
	}

	if err := durable.WriteKVFile(e.files(), filepath.Join(e.dir, m.KV), records); err != nil {
		return fmt.Errorf("cannot write the checkpoint: %w", err)
	}

	for seg, n := range docLive {
		m.Live[strconv.FormatUint(uint64(seg), 10)] = n
	}

	m.CreatedAt = time.Now().UTC().Format(time.RFC3339)
	data, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return err
	}

	if err := e.manifests.Commit(append(data, '\n')); err != nil {
		return fmt.Errorf("cannot write the manifest: %w", err)
	}

	e.previous = e.current
	e.current = &m
	e.removeOldCheckpoints()
	e.ckState.Store(&checkpointState{pos: pos, lsn: lsn, at: time.Now()})
	e.segmentLive.Store(&live)
	e.stats.checkpoints.Add(1)
	e.stats.checkpointNanos.Store(int64(time.Since(start)))

	retain := pos
	if e.previous != nil && e.previous.Format == manifestFormatPages {
		retain = journal.Position{Segment: e.previous.Segment, Offset: e.previous.Offset}
	}
	e.retireDeadSegments(retain, live)
	e.cleanPages(snaps)
	e.pages.Sweep()

	engineLog.Debug("checkpoint written", "lsn", lsn, "entities", len(snaps), "kv_keys", len(kv), "duration_ms", log.Since(start))

	return nil
}

func (e *Engine) removeOldCheckpoints() {
	keep := map[string]bool{}
	for _, m := range []*manifest{e.current, e.previous} {
		if m != nil && m.KV != "" {
			keep[m.KV] = true
		}
	}

	entries, err := e.files().ReadDir(e.dir)
	if err != nil {
		return
	}

	for _, entry := range entries {
		name := entry.Name()
		if !keep[name] && strings.HasPrefix(name, "kv-") && (strings.HasSuffix(name, ".ckpt") || strings.HasSuffix(name, ".ckpt.tmp")) {
			_ = e.files().Remove(filepath.Join(e.dir, name))
		}
	}
}

func (e *Engine) CompactPages() (int, error) {
	before := len(e.pages.SegmentSizes())
	for round := 0; round < 3; round++ {
		e.ckptMu.Lock()
		e.lastPageWalk = time.Time{}
		e.ckptMu.Unlock()

		if err := e.checkpoint(true); err != nil {
			return 0, err
		}
	}

	return before - len(e.pages.SegmentSizes()), nil
}

func (e *Engine) PageStats() durable.PageStats {
	return e.pages.Stats()
}

func (e *Engine) cleanPages(snaps []entitySnapshot) {
	if len(e.pageRemoval) == 0 && time.Since(e.lastPageWalk) < pageWalkEvery {
		return
	}
	e.lastPageWalk = time.Now()

	live := map[uint32]int64{}
	if e.previous != nil && e.previous.Format == manifestFormatPages {
		for _, r := range e.previous.roots() {
			e.openTree(r).WalkStored(func(id uint64, size uint32) {
				seg, _ := durable.SplitPageID(id)
				live[seg] += int64(size) + durable.PageHeaderSize
			})
		}
	}

	for _, snap := range snaps {
		trees := []pagetree.Tree{snap.r.slots, snap.r.ids}
		for _, idx := range snap.r.indexes {
			if idx.ready {
				trees = append(trees, idx.tree)
			}
		}

		for _, t := range trees {
			t.WalkStored(func(id uint64, size uint32) {
				seg, _ := durable.SplitPageID(id)
				live[seg] += int64(size) + durable.PageHeaderSize
			})
		}
	}

	current := e.pages.CurrentSegment()
	var dead, victims []uint32
	for seg, size := range e.pages.SegmentSizes() {
		if seg >= current {
			continue
		}

		switch {
		case live[seg] == 0:
			dead = append(dead, seg)
		case live[seg]*2 < size && len(victims) < pageVictimsPerCycle:
			victims = append(victims, seg)
		}
	}

	e.pages.Remove(dead)
	e.pageRemoval = victims

	if len(victims) == 0 {
		return
	}

	isVictim := map[uint32]bool{}
	for _, seg := range victims {
		isVictim[seg] = true
	}

	e.relocatePages(func(id uint64) bool {
		seg, _ := durable.SplitPageID(id)
		return isVictim[seg]
	})
}

func (e *Engine) relocatePages(victim func(id uint64) bool) {
	e.catalogMu.RLock()
	ents := make([]*entity, 0, len(e.entities))
	for _, ent := range e.entities {
		ents = append(ents, ent)
	}
	e.catalogMu.RUnlock()

	for _, ent := range ents {
		ent.apply.Lock()
		cur := ent.root.Load()
		tok := pagetree.NewToken()
		nr := *cur
		moved := 0

		relocate := func(t pagetree.Tree) pagetree.Tree {
			out, n := t.WithToken(tok).Relocate(victim)
			moved += n
			return out.WithToken(nil)
		}

		nr.slots = relocate(cur.slots)
		nr.ids = relocate(cur.ids)
		nr.indexes = cloneIndexes(cur.indexes)
		for name, idx := range cur.indexes {
			if idx.ready {
				nr.indexes[name] = &index{spec: idx.spec, ready: true, tree: relocate(idx.tree)}
			}
		}

		tok.Freeze()
		if moved > 0 {
			ent.root.Store(&nr)
		}
		ent.apply.Unlock()
	}
}

func (e *Engine) retireDeadSegments(pos journal.Position, live map[uint32]int64) {
	segments, err := e.j.Segments()
	if err != nil {
		return
	}

	for _, seg := range segments {
		if seg.ID >= pos.Segment {
			break
		}

		if live[seg.ID] == 0 {
			if err := e.j.Retire(seg.ID); err != nil {
				engineLog.Warn("cannot remove a log segment", "segment", seg.ID, "err", err)
			}
		}
	}
}
