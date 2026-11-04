package datastore

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
	pageHeaderSize    = 8
	pageCloseDelay    = 15 * time.Minute
)

var errPageChecksum = errors.New("datastore: page checksum mismatch")

type cachedPage struct {
	ref   weak.Pointer[pagetree.Ref]
	bytes int64
}

type pageStore struct {
	dir     string
	segSize int64

	wmu      sync.Mutex
	cur      *os.File
	curID    uint32
	curOff   int64
	buf      []byte
	bufStart int64
	unsynced map[uint32]*os.File

	rmu     sync.RWMutex
	files   map[uint32]*os.File
	sizes   map[uint32]int64
	retired map[uint32]*os.File

	cmu      sync.Mutex
	ring     []cachedPage
	hand     int
	resident atomic.Int64
	limit    atomic.Int64

	loads  atomic.Int64
	writes atomic.Int64
}

func pageSegmentName(id uint32) string {
	return fmt.Sprintf("%06d%s", id, pageSegmentSuffix)
}

func pageID(seg uint32, off int64) uint64 {
	return uint64(seg)<<32 | uint64(off)
}

func splitPageID(id uint64) (uint32, int64) {
	return uint32(id >> 32), int64(uint32(id))
}

func openPageStore(dir string, limit, segSize int64) (*pageStore, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}

	if segSize <= 0 {
		segSize = pageSegmentSize
	}

	s := &pageStore{dir: dir, segSize: segSize, files: map[uint32]*os.File{}, sizes: map[uint32]int64{}, retired: map[uint32]*os.File{}, unsynced: map[uint32]*os.File{}}
	s.limit.Store(limit)

	entries, err := os.ReadDir(dir)
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

		f, err := os.Open(filepath.Join(dir, name))
		if err != nil {
			s.close()
			return nil, err
		}

		info, err := f.Stat()
		if err != nil {
			f.Close()
			s.close()
			return nil, err
		}

		s.files[uint32(id)] = f
		s.sizes[uint32(id)] = info.Size()
		s.curID = max(s.curID, uint32(id))
	}

	return s, nil
}

func (s *pageStore) setLimit(limit int64) {
	s.limit.Store(limit)
	s.evict()
}

func (s *pageStore) Load(ref *pagetree.Ref) (*pagetree.Page, error) {
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

func (s *pageStore) read(id uint64, size uint32) ([]byte, error) {
	seg, off := splitPageID(id)

	s.wmu.Lock()
	if seg == s.curID && s.cur != nil && off >= s.bufStart {
		start := off - s.bufStart
		if start+pageHeaderSize+int64(size) <= int64(len(s.buf)) {
			b := append([]byte(nil), s.buf[start+pageHeaderSize:start+pageHeaderSize+int64(size)]...)
			s.wmu.Unlock()
			return b, nil
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

	b := make([]byte, pageHeaderSize+int(size))
	if _, err := f.ReadAt(b, off); err != nil {
		return nil, fmt.Errorf("page %x: %w", id, err)
	}

	if binary.LittleEndian.Uint32(b) != size || crc32.Checksum(b[pageHeaderSize:], castagnoli) != binary.LittleEndian.Uint32(b[4:]) {
		return nil, fmt.Errorf("page %x: %w", id, errPageChecksum)
	}

	return b[pageHeaderSize:], nil
}

func (s *pageStore) remember(ref *pagetree.Ref, p *pagetree.Page) {
	size := int64(p.Bytes())

	s.cmu.Lock()
	s.ring = append(s.ring, cachedPage{ref: weak.Make(ref), bytes: size})
	s.cmu.Unlock()

	if s.resident.Add(size) > s.limit.Load() && s.limit.Load() > 0 {
		s.evict()
	}
}

func (s *pageStore) evict() {
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

func (s *pageStore) sweep() {
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

func (s *pageStore) write(ref *pagetree.Ref, p *pagetree.Page) error {
	payload, err := pagetree.Encode(nil, p)
	if err != nil {
		return err
	}

	s.wmu.Lock()
	if s.cur == nil || s.curOff+int64(len(payload))+pageHeaderSize > s.segSize {
		if err := s.rotateLocked(); err != nil {
			s.wmu.Unlock()
			return err
		}
	}

	off := s.curOff
	var header [pageHeaderSize]byte
	binary.LittleEndian.PutUint32(header[:], uint32(len(payload)))
	binary.LittleEndian.PutUint32(header[4:], crc32.Checksum(payload, castagnoli))
	s.buf = append(append(s.buf, header[:]...), payload...)
	s.curOff += int64(len(payload)) + pageHeaderSize
	id := pageID(s.curID, off)

	if len(s.buf) >= 4<<20 {
		if err := s.flushBufLocked(); err != nil {
			s.wmu.Unlock()
			return err
		}
	}
	s.wmu.Unlock()

	ref.Persisted(id, uint32(len(payload)))
	s.writes.Add(1)

	if c := p.Compact(); ref.Replace(p, c) {
		p = c
	}
	s.remember(ref, p)

	return nil
}

func (s *pageStore) rotateLocked() error {
	if err := s.flushBufLocked(); err != nil {
		return err
	}

	id := s.curID + 1
	path := filepath.Join(s.dir, pageSegmentName(id))
	f, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_RDWR, 0o600)
	if err != nil {
		return err
	}

	if err := syncDir(s.dir); err != nil {
		f.Close()
		return err
	}

	s.rmu.Lock()
	s.files[id] = f
	s.sizes[id] = 0
	s.rmu.Unlock()

	s.cur, s.curID, s.curOff, s.bufStart = f, id, 0, 0
	s.unsynced[id] = f

	return nil
}

func (s *pageStore) flushBufLocked() error {
	if s.cur == nil || len(s.buf) == 0 {
		return nil
	}

	if _, err := s.cur.WriteAt(s.buf, s.bufStart); err != nil {
		return err
	}

	s.bufStart += int64(len(s.buf))
	s.buf = s.buf[:0]
	s.unsynced[s.curID] = s.cur

	s.rmu.Lock()
	s.sizes[s.curID] = s.bufStart
	s.rmu.Unlock()

	return nil
}

func (s *pageStore) flushTree(t pagetree.Tree) error {
	return t.ForEachDirty(func(ref *pagetree.Ref, p *pagetree.Page) error {
		return s.write(ref, p)
	})
}

func (s *pageStore) sync() error {
	s.wmu.Lock()
	defer s.wmu.Unlock()

	if err := s.flushBufLocked(); err != nil {
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

func (s *pageStore) currentSegment() uint32 {
	s.wmu.Lock()
	defer s.wmu.Unlock()

	return s.curID
}

func (s *pageStore) segmentSizes() map[uint32]int64 {
	s.rmu.RLock()
	defer s.rmu.RUnlock()

	out := make(map[uint32]int64, len(s.sizes))
	for id, size := range s.sizes {
		out[id] = size
	}

	return out
}

func (s *pageStore) remove(ids []uint32) {
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
		_ = os.Remove(filepath.Join(s.dir, pageSegmentName(id)))
	}
	s.rmu.Unlock()

	_ = syncDir(s.dir)

	time.AfterFunc(pageCloseDelay, func() {
		s.rmu.Lock()
		defer s.rmu.Unlock()
		for _, id := range retired {
			if f := s.retired[id]; f != nil {
				_ = f.Close()
				delete(s.retired, id)
			}
		}
	})
}

func (s *pageStore) close() error {
	s.wmu.Lock()
	err := s.flushBufLocked()
	for id, f := range s.unsynced {
		if serr := f.Sync(); serr != nil && err == nil {
			err = serr
		}
		delete(s.unsynced, id)
	}
	s.cur = nil
	s.wmu.Unlock()

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

func (s *pageStore) stats() PageStats {
	return PageStats{Resident: s.resident.Load(), Limit: s.limit.Load(), Loads: s.loads.Load(), Writes: s.writes.Load()}
}
