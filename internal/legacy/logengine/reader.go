package logengine

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"hash/crc32"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"
)

const (
	ManifestFile      = "MANIFEST"
	LogFolder         = "log"
	PagesFolder       = "pages"
	LegacyCheckpoints = "legacy-checkpoint"

	formatCheckpoint = 1
	formatPages      = 2

	segmentMagic      = "ELYSLOG1"
	segmentHeaderSize = 16
	segmentSuffix     = ".seg"
	pageSuffix        = ".pgs"
	pageHeaderSize    = 8
	batchMagic        = uint32(0x31424c45)
	batchHeaderSize   = 16
	maxRecordsInBatch = 1 << 24

	checkpointMagic   = "ELYCKPT1"
	kvCheckpointMagic = "ELYKVCK1"
	checkpointVersion = 1
)

const (
	kindDocPut uint8 = iota + 1
	kindDocDelete
	kindDocMove
	kindEntityReset
	kindEntityCreate
	kindEntityDrop
	kindKVPut
	kindKVDelete
	kindKVExpire
	kindKVMove
	kindKVReset
)

var (
	castagnoli = crc32.MakeTable(crc32.Castagnoli)
	errCorrupt = errors.New("log engine data is corrupt")
)

func Present(dir string) bool {
	_, err := os.Stat(filepath.Join(dir, ManifestFile))
	return err == nil
}

func FilesIn(dir string) []string {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil
	}

	out := []string{}
	for _, entry := range entries {
		name := strings.TrimSuffix(entry.Name(), ".tmp")
		switch {
		case name == ManifestFile, name == LogFolder, name == PagesFolder, name == LegacyCheckpoints:
		case strings.HasPrefix(name, "kv-") && strings.HasSuffix(name, ".ckpt"):
		case strings.HasPrefix(name, "checkpoint-") && strings.HasSuffix(name, ".ckpt"):
		default:
			continue
		}
		out = append(out, entry.Name())
	}

	return out
}

type location struct {
	segment uint32
	offset  uint32
	length  uint32
}

type slot struct {
	seq uint32
	id  string
	lsn uint64
	loc location
}

type pageRef struct {
	ID   uint64 `json:"id"`
	Size uint32 `json:"size"`
}

type refPath struct {
	Path   string `json:"Path"`
	Target string `json:"Target"`
}

type manifestEntity struct {
	Name    string    `json:"name"`
	Gen     uint64    `json:"gen"`
	NextSeq uint32    `json:"next_seq"`
	Refs    []refPath `json:"refs"`
	Slots   pageRef   `json:"slots"`
	IDs     pageRef   `json:"ids"`
}

type manifest struct {
	Format     int              `json:"format"`
	Checkpoint string           `json:"checkpoint"`
	KV         string           `json:"kv"`
	LSN        uint64           `json:"lsn"`
	Segment    uint32           `json:"segment"`
	Offset     uint32           `json:"offset"`
	Types      []string         `json:"types"`
	Entities   []manifestEntity `json:"entities"`
}

type Ref struct {
	Path   string
	Target string
}

type Document struct {
	ID    string
	Value []byte
}

type KV struct {
	Key       string
	Value     []byte
	ExpiresAt int64
}

type change struct {
	slot
	deleted bool
}

type entityState struct {
	name     string
	gen      uint64
	nextSeq  uint32
	baseNext uint32
	fromBase bool
	refs     []Ref
	slots    pageRef
	ids      pageRef
	inline   []slot
	inlineID map[string]uint32
	changes  map[uint32]*change
	byID     map[string]uint32
}

type kvEntry struct {
	lsn uint64
	exp int64
	loc location
}

type Folder struct {
	dir      string
	segments *files
	pages    *files
	types    []string
	entities map[string]*entityState
	kv       map[string]kvEntry
	replayed int
	now      int64
}

type files struct {
	dir    string
	suffix string
	open   map[uint32]*os.File
}

func (f *files) get(id uint32) (*os.File, error) {
	if h, ok := f.open[id]; ok {
		return h, nil
	}

	h, err := os.Open(filepath.Join(f.dir, fmt.Sprintf("%06d%s", id, f.suffix)))
	if err != nil {
		return nil, err
	}
	f.open[id] = h

	return h, nil
}

func (f *files) close() {
	for _, h := range f.open {
		_ = h.Close()
	}
	f.open = map[uint32]*os.File{}
}

func Open(dir string, now time.Time) (*Folder, error) {
	raw, err := os.ReadFile(filepath.Join(dir, ManifestFile))
	if err != nil {
		return nil, err
	}

	var m manifest
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("cannot read %s: %w", ManifestFile, err)
	}

	f := &Folder{
		dir:      dir,
		segments: &files{dir: filepath.Join(dir, LogFolder), suffix: segmentSuffix, open: map[uint32]*os.File{}},
		pages:    &files{dir: filepath.Join(dir, PagesFolder), suffix: pageSuffix, open: map[uint32]*os.File{}},
		entities: map[string]*entityState{},
		kv:       map[string]kvEntry{},
		now:      now.Unix(),
	}

	var from location
	switch m.Format {
	case formatPages:
		if err := f.loadPages(&m); err != nil {
			f.Close()
			return nil, err
		}
		from = location{segment: m.Segment, offset: m.Offset}
	case formatCheckpoint:
		pos, err := f.loadCheckpoint(filepath.Join(dir, m.Checkpoint))
		if err != nil {
			f.Close()
			return nil, fmt.Errorf("cannot read the checkpoint %s: %w", m.Checkpoint, err)
		}
		from = pos
	default:
		f.Close()
		return nil, fmt.Errorf("%s has format %d, formats %d and %d are read", ManifestFile, m.Format, formatCheckpoint, formatPages)
	}

	if err := f.replay(from); err != nil {
		f.Close()
		return nil, fmt.Errorf("cannot replay the log: %w", err)
	}

	return f, nil
}

func (f *Folder) Close() {
	f.segments.close()
	f.pages.close()
}

func (f *Folder) Replayed() int {
	return f.replayed
}

func (f *Folder) Types() []string {
	return append([]string(nil), f.types...)
}

func (f *Folder) Entities() []string {
	out := make([]string, 0, len(f.entities))
	for name := range f.entities {
		out = append(out, name)
	}
	sort.Strings(out)

	return out
}

func (f *Folder) Refs(entity string) []Ref {
	if st := f.entities[entity]; st != nil {
		return append([]Ref(nil), st.refs...)
	}

	return nil
}

func (f *Folder) entity(name string) *entityState {
	st := f.entities[name]
	if st == nil {
		st = &entityState{name: name, changes: map[uint32]*change{}, byID: map[string]uint32{}}
		f.entities[name] = st
	}

	return st
}

func (f *Folder) loadPages(m *manifest) error {
	f.types = append([]string(nil), m.Types...)
	for _, me := range m.Entities {
		st := f.entity(me.Name)
		st.gen = me.Gen
		st.nextSeq = me.NextSeq
		st.baseNext = me.NextSeq
		st.fromBase = true
		st.slots = me.Slots
		st.ids = me.IDs
		for _, r := range me.Refs {
			st.refs = append(st.refs, Ref{Path: r.Path, Target: r.Target})
		}
	}

	if m.KV == "" {
		return nil
	}

	entries, err := readKVCheckpoint(filepath.Join(f.dir, m.KV))
	if err != nil {
		return fmt.Errorf("cannot read the checkpoint %s: %w", m.KV, err)
	}

	for key, entry := range entries {
		f.kv[key] = entry
	}

	return nil
}

type reader struct {
	data []byte
	pos  int
	err  error
}

func (r *reader) uvarint() uint64 {
	if r.err != nil {
		return 0
	}

	v, n := binary.Uvarint(r.data[r.pos:])
	if n <= 0 {
		r.err = errCorrupt
		return 0
	}
	r.pos += n

	return v
}

func (r *reader) varint() int64 {
	if r.err != nil {
		return 0
	}

	v, n := binary.Varint(r.data[r.pos:])
	if n <= 0 {
		r.err = errCorrupt
		return 0
	}
	r.pos += n

	return v
}

func (r *reader) str() string {
	l := r.uvarint()
	if r.err != nil || uint64(len(r.data)-r.pos) < l {
		r.err = errCorrupt
		return ""
	}

	s := string(r.data[r.pos : r.pos+int(l)])
	r.pos += int(l)

	return s
}

func (r *reader) loc() location {
	return location{segment: uint32(r.uvarint()), offset: uint32(r.uvarint()), length: uint32(r.uvarint())}
}

func checkedBody(path, magic string) ([]byte, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	if len(data) < len(magic)+4 || string(data[:len(magic)]) != magic {
		return nil, errCorrupt
	}

	body := data[:len(data)-4]
	if crc32.Checksum(body, castagnoli) != binary.LittleEndian.Uint32(data[len(data)-4:]) {
		return nil, errCorrupt
	}

	return body, nil
}

func readKVCheckpoint(path string) (map[string]kvEntry, error) {
	body, err := checkedBody(path, kvCheckpointMagic)
	if err != nil {
		return nil, err
	}

	r := &reader{data: body, pos: len(kvCheckpointMagic)}
	out := map[string]kvEntry{}
	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		key := r.str()
		out[key] = kvEntry{lsn: r.uvarint(), exp: r.varint(), loc: r.loc()}
	}

	if r.err == nil && r.pos != len(r.data) {
		r.err = errCorrupt
	}

	return out, r.err
}

func (f *Folder) loadCheckpoint(path string) (location, error) {
	body, err := checkedBody(path, checkpointMagic)
	if err != nil {
		return location{}, err
	}

	r := &reader{data: body, pos: len(checkpointMagic)}
	if v := r.uvarint(); v != checkpointVersion {
		return location{}, fmt.Errorf("checkpoint version %d is not supported", v)
	}

	r.uvarint()
	pos := location{segment: uint32(r.uvarint()), offset: uint32(r.uvarint())}

	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		f.types = append(f.types, r.str())
	}

	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		st := f.entity(r.str())
		st.gen = r.uvarint()
		st.nextSeq = uint32(r.uvarint())
		st.baseNext = st.nextSeq
		st.fromBase = true
		r.uvarint()
		for m := r.uvarint(); m > 0 && r.err == nil; m-- {
			st.refs = append(st.refs, Ref{Path: r.str(), Target: r.str()})
		}

		st.inlineID = map[string]uint32{}
		for m := r.uvarint(); m > 0 && r.err == nil; m-- {
			s := slot{seq: uint32(r.uvarint()), id: r.str(), lsn: r.uvarint(), loc: r.loc()}
			st.inline = append(st.inline, s)
			st.inlineID[s.id] = s.seq
		}
		sort.Slice(st.inline, func(i, j int) bool { return st.inline[i].seq < st.inline[j].seq })

		for m := r.uvarint(); m > 0 && r.err == nil; m-- {
			r.str()
			for k := r.uvarint(); k > 0 && r.err == nil; k-- {
				r.str()
			}
		}
	}

	for n := r.uvarint(); n > 0 && r.err == nil; n-- {
		key := r.str()
		f.kv[key] = kvEntry{lsn: r.uvarint(), exp: r.varint(), loc: r.loc()}
	}

	return pos, r.err
}

type page struct {
	leaf bool
	keys [][]byte
	vals [][]byte
	kids []pageRef
}

func (f *Folder) page(ref pageRef) (*page, error) {
	h, err := f.pages.get(uint32(ref.ID >> 32))
	if err != nil {
		return nil, err
	}

	b := make([]byte, pageHeaderSize+int(ref.Size))
	if _, err := h.ReadAt(b, int64(uint32(ref.ID))); err != nil {
		return nil, fmt.Errorf("page %x: %w", ref.ID, err)
	}

	if binary.LittleEndian.Uint32(b) != ref.Size || crc32.Checksum(b[pageHeaderSize:], castagnoli) != binary.LittleEndian.Uint32(b[4:]) {
		return nil, fmt.Errorf("page %x: %w", ref.ID, errCorrupt)
	}

	return decodePage(b[pageHeaderSize:])
}

func decodePage(b []byte) (*page, error) {
	if len(b) < 2 || (b[0] != 1 && b[0] != 2) {
		return nil, errCorrupt
	}

	p := &page{leaf: b[0] == 1}
	r := &reader{data: b, pos: 1}
	n := r.uvarint()
	if n > 1<<16 {
		return nil, errCorrupt
	}

	bytesOf := func() []byte {
		l := r.uvarint()
		if r.err != nil || uint64(len(b)-r.pos) < l {
			r.err = errCorrupt
			return nil
		}
		out := b[r.pos : r.pos+int(l)]
		r.pos += int(l)
		return out
	}

	for i := uint64(0); i < n && r.err == nil; i++ {
		p.keys = append(p.keys, bytesOf())
		if p.leaf {
			p.vals = append(p.vals, bytesOf())
			continue
		}

		if len(b)-r.pos < 8 {
			return nil, errCorrupt
		}
		id := binary.LittleEndian.Uint64(b[r.pos:])
		r.pos += 8
		size := r.uvarint()
		r.uvarint()
		if id == 0 {
			return nil, errCorrupt
		}
		p.kids = append(p.kids, pageRef{ID: id, Size: uint32(size)})
	}

	if r.err != nil || r.pos != len(b) {
		return nil, errCorrupt
	}

	return p, nil
}

func (f *Folder) ascend(ref pageRef, fn func(key, val []byte) error) error {
	if ref.ID == 0 {
		return nil
	}

	p, err := f.page(ref)
	if err != nil {
		return err
	}

	if p.leaf {
		for i := range p.keys {
			if err := fn(p.keys[i], p.vals[i]); err != nil {
				return err
			}
		}
		return nil
	}

	for _, kid := range p.kids {
		if err := f.ascend(kid, fn); err != nil {
			return err
		}
	}

	return nil
}

func (f *Folder) lookup(ref pageRef, key []byte) ([]byte, bool, error) {
	for ref.ID != 0 {
		p, err := f.page(ref)
		if err != nil {
			return nil, false, err
		}

		if p.leaf {
			i := sort.Search(len(p.keys), func(i int) bool { return bytes.Compare(p.keys[i], key) >= 0 })
			if i < len(p.keys) && bytes.Equal(p.keys[i], key) {
				return p.vals[i], true, nil
			}
			return nil, false, nil
		}

		i := sort.Search(len(p.keys), func(i int) bool { return bytes.Compare(p.keys[i], key) > 0 })
		if i > 0 {
			i--
		}
		ref = p.kids[i]
	}

	return nil, false, nil
}

func decodeSlot(seq uint32, b []byte) (slot, error) {
	r := &reader{data: b}
	s := slot{seq: seq, lsn: r.uvarint()}
	s.loc = r.loc()
	if r.err != nil {
		return slot{}, r.err
	}
	s.id = string(b[r.pos:])

	return s, nil
}

func seqKey(seq uint32) []byte {
	return binary.BigEndian.AppendUint32(nil, seq)
}

func (f *Folder) baseByID(st *entityState, id string) (slot, bool, error) {
	if !st.fromBase {
		return slot{}, false, nil
	}

	if st.inlineID != nil {
		seq, ok := st.inlineID[id]
		if !ok {
			return slot{}, false, nil
		}
		i := sort.Search(len(st.inline), func(i int) bool { return st.inline[i].seq >= seq })
		return st.inline[i], true, nil
	}

	sb, ok, err := f.lookup(st.ids, []byte(id))
	if err != nil || !ok || len(sb) != 4 {
		return slot{}, false, err
	}

	seq := binary.BigEndian.Uint32(sb)
	v, ok, err := f.lookup(st.slots, seqKey(seq))
	if err != nil || !ok {
		return slot{}, false, err
	}

	s, err := decodeSlot(seq, v)

	return s, err == nil, err
}

func (f *Folder) current(st *entityState, id string) (slot, bool, error) {
	if seq, ok := st.byID[id]; ok {
		c := st.changes[seq]
		if c == nil || c.deleted {
			return slot{}, false, nil
		}
		return c.slot, true, nil
	}

	s, ok, err := f.baseByID(st, id)
	if err != nil || !ok {
		return slot{}, false, err
	}

	if c, changed := st.changes[s.seq]; changed {
		if c.deleted {
			return slot{}, false, nil
		}
		return c.slot, true, nil
	}

	return s, true, nil
}

type record struct {
	kind   uint8
	lsn    uint64
	gen    uint64
	exp    int64
	entity string
	key    string
	loc    location
}

func decodeBatch(data []byte, segment uint32, base uint32) ([]record, int, error) {
	if len(data) < batchHeaderSize {
		return nil, 0, errCorrupt
	}

	if binary.LittleEndian.Uint32(data[0:]) != batchMagic {
		return nil, 0, errCorrupt
	}

	payloadLen := int(binary.LittleEndian.Uint32(data[4:]))
	if batchHeaderSize+payloadLen > len(data) {
		return nil, 0, errCorrupt
	}

	payload := data[batchHeaderSize : batchHeaderSize+payloadLen]
	if crc32.Checksum(payload, castagnoli) != binary.LittleEndian.Uint32(data[8:]) {
		return nil, 0, errCorrupt
	}

	count := int(binary.LittleEndian.Uint32(data[12:]))
	if count > maxRecordsInBatch {
		return nil, 0, errCorrupt
	}

	recs := make([]record, 0, count)
	pos := 0
	for i := 0; i < count; i++ {
		if len(payload)-pos < 9 {
			return nil, 0, errCorrupt
		}

		r := &reader{data: payload, pos: pos + 9}
		rec := record{kind: payload[pos], lsn: binary.LittleEndian.Uint64(payload[pos+1:])}
		rec.gen = r.uvarint()
		rec.exp = r.varint()
		rec.entity = r.str()
		rec.key = r.str()
		valueLen := r.uvarint()
		if r.err != nil || uint64(len(payload)-r.pos) < valueLen {
			return nil, 0, errCorrupt
		}

		rec.loc = location{segment: segment, offset: base + uint32(batchHeaderSize+r.pos), length: uint32(valueLen)}
		pos = r.pos + int(valueLen)
		recs = append(recs, rec)
	}

	if pos != len(payload) {
		return nil, 0, errCorrupt
	}

	return recs, batchHeaderSize + payloadLen, nil
}

func (f *Folder) segmentIDs() ([]uint32, error) {
	entries, err := os.ReadDir(f.segments.dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}

	ids := []uint32{}
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasSuffix(name, segmentSuffix) {
			continue
		}

		id, err := strconv.ParseUint(strings.TrimSuffix(name, segmentSuffix), 10, 32)
		if err == nil {
			ids = append(ids, uint32(id))
		}
	}
	slices.Sort(ids)

	return ids, nil
}

func (f *Folder) replay(from location) error {
	ids, err := f.segmentIDs()
	if err != nil {
		return err
	}

	for idx, id := range ids {
		if id < from.segment {
			continue
		}

		last := idx == len(ids)-1
		data, err := os.ReadFile(filepath.Join(f.segments.dir, fmt.Sprintf("%06d%s", id, segmentSuffix)))
		if err != nil {
			return err
		}

		if len(data) < segmentHeaderSize {
			if last {
				return nil
			}
			return fmt.Errorf("segment %d is %d bytes long", id, len(data))
		}

		if string(data[:8]) != segmentMagic || binary.LittleEndian.Uint32(data[8:]) != id {
			return fmt.Errorf("segment %d: %w", id, errCorrupt)
		}

		pos := segmentHeaderSize
		if id == from.segment && int(from.offset) > pos {
			pos = int(from.offset)
		}

		for pos < len(data) {
			recs, n, err := decodeBatch(data[pos:], id, uint32(pos))
			if err != nil {
				if last {
					break
				}
				return fmt.Errorf("segment %d offset %d: %w", id, pos, err)
			}

			for i := range recs {
				f.apply(&recs[i])
			}
			f.replayed += len(recs)
			pos += n
		}
	}

	return nil
}

func (f *Folder) apply(rec *record) {
	switch rec.kind {
	case kindDocPut, kindDocMove:
		st := f.entity(rec.entity)
		if rec.gen != st.gen {
			return
		}

		old, exists, err := f.current(st, rec.key)
		if err != nil || (exists && rec.lsn <= old.lsn) || (!exists && rec.kind == kindDocMove) {
			return
		}

		s := slot{id: rec.key, lsn: rec.lsn, loc: rec.loc}
		if exists {
			s.seq = old.seq
		} else {
			s.seq = st.nextSeq
			st.nextSeq++
		}
		st.changes[s.seq] = &change{slot: s}
		st.byID[s.id] = s.seq
	case kindDocDelete:
		st := f.entity(rec.entity)
		if rec.gen != st.gen {
			return
		}

		old, exists, err := f.current(st, rec.key)
		if err != nil || !exists || old.lsn >= rec.lsn {
			return
		}
		st.changes[old.seq] = &change{slot: old, deleted: true}
		st.byID[old.id] = old.seq
	case kindEntityReset:
		st := f.entity(rec.entity)
		if rec.gen <= st.gen {
			return
		}
		*st = entityState{name: st.name, gen: rec.gen, changes: map[uint32]*change{}, byID: map[string]uint32{}}
	case kindEntityCreate:
		if !slices.Contains(f.types, rec.entity) {
			f.types = append(f.types, rec.entity)
		}
	case kindEntityDrop:
		f.types = slices.DeleteFunc(f.types, func(t string) bool { return t == rec.entity })
	case kindKVPut, kindKVMove:
		old, exists := f.kv[rec.key]
		if exists && old.lsn >= rec.lsn {
			return
		}

		if rec.kind == kindKVMove {
			if exists {
				old.lsn, old.loc = rec.lsn, rec.loc
				f.kv[rec.key] = old
			}
			return
		}

		if rec.exp > 0 && rec.exp <= f.now {
			delete(f.kv, rec.key)
			return
		}
		f.kv[rec.key] = kvEntry{lsn: rec.lsn, exp: rec.exp, loc: rec.loc}
	case kindKVDelete, kindKVExpire:
		delete(f.kv, rec.key)
	case kindKVReset:
		f.kv = map[string]kvEntry{}
	}
}

func (f *Folder) value(loc location) ([]byte, error) {
	out := make([]byte, loc.length)
	if loc.length == 0 {
		return out, nil
	}

	h, err := f.segments.get(loc.segment)
	if err != nil {
		return nil, err
	}

	if _, err := h.ReadAt(out, int64(loc.offset)); err != nil {
		return nil, fmt.Errorf("segment %d: %w", loc.segment, err)
	}

	return out, nil
}

func (f *Folder) Each(entity string, fn func(doc Document) error) error {
	st := f.entities[entity]
	if st == nil {
		return nil
	}

	emit := func(s slot) error {
		if c, ok := st.changes[s.seq]; ok {
			if c.deleted {
				return nil
			}
			s = c.slot
		}

		v, err := f.value(s.loc)
		if err != nil {
			return err
		}

		return fn(Document{ID: s.id, Value: v})
	}

	if st.fromBase {
		if st.inlineID != nil {
			for _, s := range st.inline {
				if err := emit(s); err != nil {
					return err
				}
			}
		} else {
			err := f.ascend(st.slots, func(key, val []byte) error {
				if len(key) != 4 {
					return errCorrupt
				}
				s, err := decodeSlot(binary.BigEndian.Uint32(key), val)
				if err != nil {
					return err
				}
				return emit(s)
			})
			if err != nil {
				return err
			}
		}
	}

	added := []uint32{}
	for seq := range st.changes {
		if !st.fromBase || seq >= st.baseNext {
			added = append(added, seq)
		}
	}
	slices.Sort(added)

	for _, seq := range added {
		c := st.changes[seq]
		if c.deleted {
			continue
		}

		v, err := f.value(c.loc)
		if err != nil {
			return err
		}

		if err := fn(Document{ID: c.id, Value: v}); err != nil {
			return err
		}
	}

	return nil
}

func (f *Folder) KV(fn func(kv KV) error) error {
	keys := make([]string, 0, len(f.kv))
	for key, entry := range f.kv {
		if entry.exp > 0 && entry.exp <= f.now {
			continue
		}
		keys = append(keys, key)
	}
	sort.Strings(keys)

	for _, key := range keys {
		entry := f.kv[key]
		v, err := f.value(entry.loc)
		if err != nil {
			return err
		}

		if err := fn(KV{Key: key, Value: v, ExpiresAt: entry.exp}); err != nil {
			return err
		}
	}

	return nil
}
