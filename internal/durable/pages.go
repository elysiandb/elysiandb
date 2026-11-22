package durable

import (
	"encoding/binary"
	"errors"
	"fmt"
	"hash/crc32"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"weak"

	"github.com/taymour/elysiandb/internal/pagetree"
)

const (
	PagesFolder       = "pages"
	pageSegmentSuffix = ".pgs"
	pageSegmentSize   = 64 << 20
	PageHeaderSize    = 8
	pageCloseDelay    = 5 * time.Minute
	pageFlushBytes    = 4 << 20
)

var (
	ErrPageChecksum = errors.New("durable: page checksum mismatch")
	castagnoli      = crc32.MakeTable(crc32.Castagnoli)
)

type cachedPage struct {
	ref   weak.Pointer[pagetree.Ref]
	bytes int64
}

type PageStore struct {
	fs      FS
	dir     string
	segSize int64

	closeDelay time.Duration

	wmu        sync.Mutex
	cur        File
	curID      uint32
	curOff     int64
	buf        []byte
	bufStart   int64
	flushing   []byte
	flushStart int64
	spare      []byte

	ioMu     sync.Mutex
	unsynced map[uint32]File
	pace     func(synced time.Duration) time.Duration

	rmu     sync.RWMutex
	files   map[uint32]File
	sizes   map[uint32]int64
	retired map[uint32]File

	cmu      sync.Mutex
	ring     []cachedPage
	hand     int
	resident atomic.Int64
	limit    atomic.Int64

	loads  atomic.Int64
	writes atomic.Int64
	holds  atomic.Int32
}

func pageSegmentName(id uint32) string {
	return fmt.Sprintf("%06d%s", id, pageSegmentSuffix)
}

func pageID(seg uint32, off int64) uint64 {
	return uint64(seg)<<32 | uint64(off)
}

func SplitPageID(id uint64) (uint32, int64) {
	return uint32(id >> 32), int64(uint32(id))
}

func OpenPageStore(fs FS, dir string, limit, segSize int64) (*PageStore, error) {
	fs = Or(fs)
	if err := fs.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}

	if segSize <= 0 {
		segSize = pageSegmentSize
	}

	s := &PageStore{fs: fs, dir: dir, segSize: segSize, closeDelay: pageCloseDelay, files: map[uint32]File{}, sizes: map[uint32]int64{}, retired: map[uint32]File{}, unsynced: map[uint32]File{}}
	s.limit.Store(limit)

	entries, err := fs.ReadDir(dir)
	if err != nil {
		return nil, err
	}

	for _, e := range entries {
		name := e.Name()
		if !strings.HasSuffix(name, pageSegmentSuffix) {
			continue
		}

		id, err := strconv.ParseUint(strings.TrimSuffix(name, pageSegmentSuffix), 10, 32)
		if err != nil {
			continue
		}

		f, err := fs.OpenFile(filepath.Join(dir, name), os.O_RDWR, 0o600)
		if err != nil {
			s.Close()
			return nil, err
		}

		info, err := f.Stat()
		if err != nil {
			f.Close()
			s.Close()
			return nil, err
		}

		s.files[uint32(id)] = f
		s.sizes[uint32(id)] = info.Size()
		s.curID = max(s.curID, uint32(id))
	}

	return s, nil
}

func (s *PageStore) CloseRetiredAfter(d time.Duration) {
	if d > 0 {
		s.closeDelay = d
	}
}

func (s *PageStore) Pace(fn func(synced time.Duration) time.Duration) {
	s.ioMu.Lock()
	s.pace = fn
	s.ioMu.Unlock()
}

func (s *PageStore) SetLimit(limit int64) {
	s.limit.Store(limit)
	s.evict()
}

func (s *PageStore) Hold() {
	s.holds.Add(1)
}

func (s *PageStore) Release() {
	s.holds.Add(-1)
}

func (s *PageStore) Peek(ref *pagetree.Ref) (*pagetree.Page, error) {
	if p := ref.Resident(); p != nil {
		return p, nil
	}

	b, err := s.read(ref.ID(), ref.Size())
	if err != nil {
		return nil, err
	}

	p, err := pagetree.Decode(b)
	if err != nil {
		return nil, fmt.Errorf("page %x: %w", ref.ID(), err)
	}

	return p, nil
}

func (s *PageStore) Load(ref *pagetree.Ref) (*pagetree.Page, error) {
	if p := ref.Resident(); p != nil {
		return p, nil
	}

	b, err := s.read(ref.ID(), ref.Size())
	if err != nil {
		return nil, err
	}

	p, err := pagetree.Decode(b)
	if err != nil {
		return nil, fmt.Errorf("page %x: %w", ref.ID(), err)
	}

	s.loads.Add(1)
	got := ref.Attach(p)
	if got == p {
		s.remember(ref, p)
	}

	return got, nil
}

func (s *PageStore) read(id uint64, size uint32) ([]byte, error) {
	seg, off := SplitPageID(id)

	s.wmu.Lock()
	if seg == s.curID && s.cur != nil {
		if off >= s.bufStart {
			start := off - s.bufStart
			if start+PageHeaderSize+int64(size) <= int64(len(s.buf)) {
				b := append([]byte(nil), s.buf[start+PageHeaderSize:start+PageHeaderSize+int64(size)]...)
				s.wmu.Unlock()
				return b, nil
			}
		} else if s.flushing != nil && off >= s.flushStart {
			start := off - s.flushStart
			if start+PageHeaderSize+int64(size) <= int64(len(s.flushing)) {
				b := append([]byte(nil), s.flushing[start+PageHeaderSize:start+PageHeaderSize+int64(size)]...)
				s.wmu.Unlock()
				return b, nil
			}
		}
	}
	s.wmu.Unlock()

	s.rmu.RLock()
	f := s.files[seg]
	if f == nil {
		f = s.retired[seg]
	}
	s.rmu.RUnlock()
	if f == nil {
		return nil, fmt.Errorf("page segment %d is missing", seg)
	}

	b := make([]byte, PageHeaderSize+int(size))
	if _, err := f.ReadAt(b, off); err != nil {
		return nil, fmt.Errorf("page %x: %w", id, err)
	}

	if binary.LittleEndian.Uint32(b) != size || crc32.Checksum(b[PageHeaderSize:], castagnoli) != binary.LittleEndian.Uint32(b[4:]) {
		return nil, fmt.Errorf("page %x: %w", id, ErrPageChecksum)
	}

	return b[PageHeaderSize:], nil
}

func (s *PageStore) remember(ref *pagetree.Ref, p *pagetree.Page) {
	size := int64(p.Bytes())

	s.cmu.Lock()
	s.ring = append(s.ring, cachedPage{ref: weak.Make(ref), bytes: size})
	s.cmu.Unlock()

	if s.resident.Add(size) > s.limit.Load() && s.limit.Load() > 0 {
		s.evict()
	}
}

func (s *PageStore) evict() {
	limit := s.limit.Load()
	if limit <= 0 {
		return
	}

	s.cmu.Lock()
	defer s.cmu.Unlock()

	for spins := 0; s.resident.Load() > limit && len(s.ring) > 0 && spins < 2*len(s.ring)+2; spins++ {
		if s.hand >= len(s.ring) {
			s.hand = 0
		}

		e := s.ring[s.hand]
		ref := e.ref.Value()
		if ref != nil && ref.TakeHot() {
			s.hand++
			continue
		}

		if ref != nil {
			ref.Evict()
		}

		s.resident.Add(-e.bytes)
		last := len(s.ring) - 1
		s.ring[s.hand] = s.ring[last]
		s.ring[last] = cachedPage{}
		s.ring = s.ring[:last]
	}
}

func (s *PageStore) Sweep() {
	s.cmu.Lock()
	defer s.cmu.Unlock()

	kept := s.ring[:0]
	for _, e := range s.ring {
		if ref := e.ref.Value(); ref != nil && ref.Resident() != nil {
			kept = append(kept, e)
			continue
		}
		s.resident.Add(-e.bytes)
	}

	clear(s.ring[len(kept):])
	s.ring = kept
	s.hand = 0
}

func (s *PageStore) Write(ref *pagetree.Ref, p *pagetree.Page) error {
	payload, err := pagetree.Encode(nil, p)
	if err != nil {
		return err
	}

	s.ioMu.Lock()
	defer s.ioMu.Unlock()

	s.wmu.Lock()
	rotate := s.cur == nil || s.curOff+int64(len(payload))+PageHeaderSize > s.segSize
	s.wmu.Unlock()

	if rotate {
		if err := s.rotate(); err != nil {
			return err
		}
	}

	var header [PageHeaderSize]byte
	binary.LittleEndian.PutUint32(header[:], uint32(len(payload)))
	binary.LittleEndian.PutUint32(header[4:], crc32.Checksum(payload, castagnoli))

	s.wmu.Lock()
	off := s.curOff
	s.buf = append(append(s.buf, header[:]...), payload...)
	s.curOff += int64(len(payload)) + PageHeaderSize
	id := pageID(s.curID, off)
	full := len(s.buf) >= pageFlushBytes
	s.wmu.Unlock()

	if full {
		if err := s.flush(); err != nil {
			return err
		}
		s.settle()
	}

	ref.Persisted(id, uint32(len(payload)))
	s.writes.Add(1)

	if c := p.Compact(); ref.Replace(p, c) {
		p = c
	}
	s.remember(ref, p)

	return nil
}

func (s *PageStore) settle() {
	if s.pace == nil {
		return
	}

	s.wmu.Lock()
	id, f := s.curID, s.cur
	s.wmu.Unlock()

	start := time.Now()
	if err := f.Sync(); err != nil {
		return
	}
	took := time.Since(start)
	delete(s.unsynced, id)

	if pause := s.pace(took); pause > 0 {
		time.Sleep(pause)
	}
}

func (s *PageStore) rotate() error {
	if err := s.flush(); err != nil {
		return err
	}

	s.wmu.Lock()
	id, previous := s.curID+1, s.cur
	s.wmu.Unlock()

	path := filepath.Join(s.dir, pageSegmentName(id))
	f, err := s.fs.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_RDWR, 0o600)
	if err != nil {
		return err
	}

	if err := s.fs.SyncDir(s.dir); err != nil {
		f.Close()
		return err
	}

	if previous != nil {
		TrimPreallocated(previous)
	}
	Preallocate(f, s.dir, s.segSize)

	s.rmu.Lock()
	s.files[id] = f
	s.sizes[id] = 0
	s.rmu.Unlock()

	s.wmu.Lock()
	s.cur, s.curID, s.curOff, s.bufStart = f, id, 0, 0
	s.wmu.Unlock()
	s.unsynced[id] = f

	return nil
}

func (s *PageStore) flush() error {
	s.wmu.Lock()
	if s.cur == nil || len(s.buf) == 0 {
		s.wmu.Unlock()
		return nil
	}

	data, start, seg, f := s.buf, s.bufStart, s.curID, s.cur
	s.flushing, s.flushStart = data, start
	s.buf = s.spare[:0]
	s.spare = nil
	s.bufStart = start + int64(len(data))
	s.wmu.Unlock()

	_, err := f.WriteAt(data, start)

	s.wmu.Lock()
	s.flushing = nil
	if err != nil {
		s.buf = append(data, s.buf...)
		s.bufStart = start
		s.wmu.Unlock()
		return err
	}
	s.spare = data[:0]
	s.wmu.Unlock()

	s.unsynced[seg] = f

	s.rmu.Lock()
	s.sizes[seg] = start + int64(len(data))
	s.rmu.Unlock()

	return nil
}

func (s *PageStore) Rotate() error {
	s.ioMu.Lock()
	defer s.ioMu.Unlock()

	s.wmu.Lock()
	empty := s.cur == nil || s.curOff == 0
	s.wmu.Unlock()
	if empty {
		return nil
	}

	return s.rotate()
}

func (s *PageStore) FlushTree(t pagetree.Tree) error {
	return t.ForEachDirty(func(ref *pagetree.Ref, p *pagetree.Page) error {
		return s.Write(ref, p)
	})
}

func (s *PageStore) Sync() error {
	s.ioMu.Lock()
	defer s.ioMu.Unlock()

	if err := s.flush(); err != nil {
		return err
	}

	for id, f := range s.unsynced {
		if err := f.Sync(); err != nil {
			return err
		}
		delete(s.unsynced, id)
	}

	return nil
}

func (s *PageStore) CurrentSegment() uint32 {
	s.wmu.Lock()
	defer s.wmu.Unlock()

	return s.curID
}

func (s *PageStore) SegmentSizes() map[uint32]int64 {
	s.rmu.RLock()
	defer s.rmu.RUnlock()

	out := make(map[uint32]int64, len(s.sizes))
	for id, size := range s.sizes {
		out[id] = size
	}

	return out
}

func (s *PageStore) Remove(ids []uint32) {
	if len(ids) == 0 {
		return
	}

	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })

	var retired []uint32
	s.rmu.Lock()
	for _, id := range ids {
		if f := s.files[id]; f != nil {
			s.retired[id] = f
			retired = append(retired, id)
		}
		delete(s.files, id)
		delete(s.sizes, id)
		_ = s.fs.Remove(filepath.Join(s.dir, pageSegmentName(id)))
	}
	s.rmu.Unlock()

	_ = s.fs.SyncDir(s.dir)

	s.closeLater(retired)
}

func (s *PageStore) closeLater(ids []uint32) {
	time.AfterFunc(s.closeDelay, func() {
		if s.holds.Load() > 0 {
			s.closeLater(ids)
			return
		}

		s.rmu.Lock()
		defer s.rmu.Unlock()
		for _, id := range ids {
			if f := s.retired[id]; f != nil {
				_ = f.Close()
				delete(s.retired, id)
			}
		}
	})
}

func (s *PageStore) Close() error {
	s.ioMu.Lock()
	err := s.flush()
	for id, f := range s.unsynced {
		if serr := f.Sync(); serr != nil && err == nil {
			err = serr
		}
		delete(s.unsynced, id)
	}
	s.wmu.Lock()
	if s.cur != nil {
		TrimPreallocated(s.cur)
	}
	s.cur = nil
	s.wmu.Unlock()
	s.ioMu.Unlock()

	s.rmu.Lock()
	for id, f := range s.files {
		_ = f.Close()
		delete(s.files, id)
	}
	for id, f := range s.retired {
		_ = f.Close()
		delete(s.retired, id)
	}
	s.rmu.Unlock()

	return err
}

type PageStats struct {
	Resident int64
	Limit    int64
	Loads    int64
	Writes   int64
}

func (s *PageStore) Stats() PageStats {
	return PageStats{Resident: s.resident.Load(), Limit: s.limit.Load(), Loads: s.loads.Load(), Writes: s.writes.Load()}
}
