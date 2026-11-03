package datastore

import (
	"bufio"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"hash"
	"hash/crc32"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/ptree"
)

const (
	checkpointMagic   = "ELYCKPT1"
	checkpointVersion = 1
	manifestFormat    = 1
)

var (
	castagnoli    = crc32.MakeTable(crc32.Castagnoli)
	errCheckpoint = errors.New("checkpoint is corrupt")
)

type manifest struct {
	Format     int    `json:"format"`
	Checkpoint string `json:"checkpoint"`
	LSN        uint64 `json:"lsn"`
	Segment    uint32 `json:"segment"`
	Offset     uint32 `json:"offset"`
	CreatedAt  string `json:"created_at"`
	Version    string `json:"elysiandb_version,omitempty"`
}

func readManifest(dir string) (*manifest, error) {
	data, err := os.ReadFile(filepath.Join(dir, ManifestFile))
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}

		return nil, err
	}

	var m manifest
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("cannot read %s: %w", ManifestFile, err)
	}

	if m.Format != manifestFormat {
		return nil, fmt.Errorf("%s has format %d, this version reads format %d", ManifestFile, m.Format, manifestFormat)
	}

	return &m, nil
}

func writeFileAtomic(path string, write func(w io.Writer) error) error {
	tmp := path + ".tmp"
	f, err := os.OpenFile(tmp, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}

	bw := bufio.NewWriterSize(f, 1<<20)
	err = write(bw)
	if err == nil {
		err = bw.Flush()
	}

	if err == nil {
		err = f.Sync()
	}

	if closeErr := f.Close(); err == nil {
		err = closeErr
	}

	if err != nil {
		_ = os.Remove(tmp)
		return err
	}

	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return err
	}

	return syncDir(filepath.Dir(path))
}

func syncDir(dir string) error {
	d, err := os.Open(dir)
	if err != nil {
		return err
	}

	defer d.Close()

	_ = d.Sync()

	return nil
}

func writeManifest(dir string, m manifest) error {
	data, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return err
	}

	return writeFileAtomic(filepath.Join(dir, ManifestFile), func(w io.Writer) error {
		_, err := w.Write(append(data, '\n'))
		return err
	})
}

type ckWriter struct {
	w   io.Writer
	crc hash.Hash32
	buf []byte
	err error
}

func newCkWriter(w io.Writer) *ckWriter {
	crc := crc32.New(castagnoli)
	return &ckWriter{w: io.MultiWriter(w, crc), crc: crc, buf: make([]byte, 0, 64)}
}

func (c *ckWriter) uvarint(v uint64) {
	if c.err != nil {
		return
	}

	c.buf = binary.AppendUvarint(c.buf[:0], v)
	_, c.err = c.w.Write(c.buf)
}

func (c *ckWriter) varint(v int64) {
	if c.err != nil {
		return
	}

	c.buf = binary.AppendVarint(c.buf[:0], v)
	_, c.err = c.w.Write(c.buf)
}

func (c *ckWriter) str(s string) {
	c.uvarint(uint64(len(s)))
	if c.err == nil {
		_, c.err = io.WriteString(c.w, s)
	}
}

func (c *ckWriter) loc(l journal.Location) {
	c.uvarint(uint64(l.Segment))
	c.uvarint(uint64(l.Offset))
	c.uvarint(uint64(l.Length))
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

	if r.err != nil {
		return nil, r.err
	}

	return ck, nil
}

type segmentCache struct {
	j    *journal.Journal
	id   uint32
	data []byte
}

func (c *segmentCache) value(loc journal.Location) ([]byte, error) {
	if loc.Length == 0 {
		return []byte{}, nil
	}

	if c.data == nil || c.id != loc.Segment {
		data, err := readSegmentFile(c.j.Dir(), loc.Segment)
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

func readSegmentFile(dir string, id uint32) ([]byte, error) {
	return os.ReadFile(filepath.Join(dir, journal.SegmentName(id)))
}

func (e *Engine) restore(ck *checkpointData) error {
	e.catalogMu.Lock()
	e.types = append([]string(nil), ck.types...)
	e.catalogMu.Unlock()

	needValues := e.inline
	var loads []*slot

	for _, ce := range ck.entities {
		ent := e.entity(ce.name, true)

		seqKeys := make([]string, len(ce.slots))
		for i, s := range ce.slots {
			seqKeys[i] = seqKey(s.seq)
			if needValues {
				loads = append(loads, s)
			}
		}

		byID := append([]*slot(nil), ce.slots...)
		sort.Slice(byID, func(i, j int) bool { return byID[i].id < byID[j].id })
		idKeys := make([]string, len(byID))
		for i, s := range byID {
			idKeys[i] = s.id
		}

		r := &root{
			gen:     ce.gen,
			nextSeq: ce.nextSeq,
			version: ce.version,
			refs:    ce.refs,
			slots:   ptree.FromSorted(seqKeys, ce.slots),
			ids:     ptree.FromSorted(idKeys, byID),
			indexes: map[string]*index{},
		}

		for _, ci := range ce.indexes {
			r.indexes[ci.spec.Name()] = &index{spec: ci.spec, ready: true, tree: ptree.FromSorted(ci.keys, make([]struct{}, len(ci.keys)))}
		}

		ent.root.Store(r)
	}

	sort.Slice(loads, func(i, j int) bool {
		if loads[i].loc.Segment != loads[j].loc.Segment {
			return loads[i].loc.Segment < loads[j].loc.Segment
		}
		return loads[i].loc.Offset < loads[j].loc.Offset
	})

	kvLoads := append([]ckKV(nil), ck.kv...)
	sort.Slice(kvLoads, func(i, j int) bool {
		a, b := kvLoads[i].entry.loc, kvLoads[j].entry.loc
		if a.Segment != b.Segment {
			return a.Segment < b.Segment
		}
		return a.Offset < b.Offset
	})

	sc := &segmentCache{j: e.j}
	for _, s := range loads {
		v, err := sc.value(s.loc)
		if err != nil {
			return err
		}
		s.data = v
	}

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

type entitySnapshot struct {
	name string
	r    *root
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
	if !force && pos == e.checkpointPos && !e.checkpointAt.IsZero() {
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
		snaps = append(snaps, entitySnapshot{name: ent.name, r: ent.root.Load()})
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

	if err := e.j.Flush(); err != nil {
		return fmt.Errorf("cannot flush the log before a checkpoint: %w", err)
	}

	live := map[uint32]int64{}
	name := fmt.Sprintf("checkpoint-%016d.ckpt", lsn)
	err := writeFileAtomic(filepath.Join(e.dir, name), func(w io.Writer) error {
		cw := newCkWriter(w)
		if _, err := io.WriteString(cw.w, checkpointMagic); err != nil {
			return err
		}

		cw.uvarint(checkpointVersion)
		cw.uvarint(lsn)
		cw.uvarint(uint64(pos.Segment))
		cw.uvarint(uint64(pos.Offset))
		cw.uvarint(uint64(len(types)))
		for _, t := range types {
			cw.str(t)
		}

		cw.uvarint(uint64(len(snaps)))
		for _, s := range snaps {
			cw.str(s.name)
			cw.uvarint(s.r.gen)
			cw.uvarint(uint64(s.r.nextSeq))
			cw.uvarint(s.r.version)
			cw.uvarint(uint64(len(s.r.refs)))
			for _, ref := range s.r.refs {
				cw.str(ref.Path)
				cw.str(ref.Target)
			}

			cw.uvarint(uint64(s.r.slots.Len()))
			s.r.slots.AscendAt(0, func(_ string, sl *slot) bool {
				cw.uvarint(uint64(sl.seq))
				cw.str(sl.id)
				cw.uvarint(sl.lsn)
				cw.loc(sl.loc)
				live[sl.loc.Segment] += int64(sl.loc.Length)
				return cw.err == nil
			})

			ready := []*index{}
			for _, idx := range s.r.indexes {
				if idx.ready {
					ready = append(ready, idx)
				}
			}
			sort.Slice(ready, func(i, j int) bool { return ready[i].spec.Name() < ready[j].spec.Name() })

			cw.uvarint(uint64(len(ready)))
			for _, idx := range ready {
				cw.str(idx.spec.Name())
				cw.uvarint(uint64(idx.tree.Len()))
				idx.tree.AscendAt(0, func(k string, _ struct{}) bool {
					cw.str(k)
					return cw.err == nil
				})
			}
		}

		cw.uvarint(uint64(len(kv)))
		for _, item := range kv {
			cw.str(item.key)
			cw.uvarint(item.entry.lsn)
			cw.varint(item.entry.expiresAt)
			cw.loc(item.entry.loc)
			live[item.entry.loc.Segment] += int64(item.entry.loc.Length)
		}

		if cw.err != nil {
			return cw.err
		}

		var sum [4]byte
		binary.LittleEndian.PutUint32(sum[:], cw.crc.Sum32())
		_, err := w.Write(sum[:])

		return err
	})
	if err != nil {
		return fmt.Errorf("cannot write the checkpoint: %w", err)
	}

	m := manifest{Format: manifestFormat, Checkpoint: name, LSN: lsn, Segment: pos.Segment, Offset: pos.Offset, CreatedAt: time.Now().UTC().Format(time.RFC3339)}
	if err := writeManifest(e.dir, m); err != nil {
		return fmt.Errorf("cannot write the manifest: %w", err)
	}

	e.removeOldCheckpoints(name)
	e.checkpointPos = pos
	e.checkpointLSN = lsn
	e.checkpointAt = time.Now()
	e.segmentLive.Store(&live)
	e.stats.checkpoints.Add(1)
	e.stats.checkpointNanos.Store(int64(time.Since(start)))

	e.retireDeadSegments(pos, live)

	engineLog.Debug("checkpoint written", "lsn", lsn, "entities", len(snaps), "kv_keys", len(kv), "duration_ms", log.Since(start))

	return nil
}

func (e *Engine) removeOldCheckpoints(keep string) {
	entries, err := os.ReadDir(e.dir)
	if err != nil {
		return
	}

	for _, entry := range entries {
		name := entry.Name()
		if name != keep && strings.HasPrefix(name, "checkpoint-") && (strings.HasSuffix(name, ".ckpt") || strings.HasSuffix(name, ".ckpt.tmp")) {
			_ = os.Remove(filepath.Join(e.dir, name))
		}
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
