package journal

import (
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"github.com/taymour/elysiandb/internal/durable"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

type Mode int

const (
	ModeBackground Mode = iota
	ModeWrite
	ModeSync
)

const (
	DefaultSegmentSize = 64 << 20
	defaultMaxPending  = 16 << 20
	retireGrace        = 2 * time.Minute
	segmentSuffix      = ".seg"
	damagedSuffix      = ".damaged"
)

var ErrClosed = errors.New("journal: closed")

type Options struct {
	Mode          Mode
	SegmentSize   int64
	FlushInterval time.Duration
	MaxPending    int
	FS            FS
	OnWriteError  func(error)
	SyncBytes     int64
}

type chunk struct {
	segment uint32
	offset  uint32
	data    []byte
	fresh   bool
}

type retiredFile struct {
	id    uint32
	file  File
	since time.Time
}

type Journal struct {
	dir  string
	opts Options
	fs   FS

	mu         sync.Mutex
	done       *sync.Cond
	nextLSN    uint64
	active     uint32
	activeSize uint32
	pending    []*chunk
	inflight   []*chunk
	appended   uint64
	written    uint64
	pendingLen int
	writeErr   error
	attempts   uint64
	closed     bool
	started    atomic.Bool
	flushed    atomic.Uint64

	kick    chan struct{}
	stop    chan struct{}
	stopped chan struct{}

	holds    atomic.Int32
	syncMu   sync.Mutex
	syncKick chan struct{}
	syncDone chan struct{}
	unsynced uint64

	filesMu sync.RWMutex
	files   map[uint32]File
	dirty   map[uint32]bool
	retired []retiredFile
}

type Ticket struct {
	j  *Journal
	at uint64
}

func SegmentName(id uint32) string {
	return fmt.Sprintf("%06d%s", id, segmentSuffix)
}

func Open(dir string, opts Options) (*Journal, error) {
	if opts.FS == nil {
		opts.FS = OSFS
	}

	if opts.SegmentSize <= 0 {
		opts.SegmentSize = DefaultSegmentSize
	}

	if opts.MaxPending <= 0 {
		opts.MaxPending = defaultMaxPending
	}

	if err := opts.FS.MkdirAll(dir, 0o700); err != nil {
		return nil, err
	}

	j := &Journal{
		dir:      dir,
		opts:     opts,
		fs:       opts.FS,
		nextLSN:  1,
		kick:     make(chan struct{}, 1),
		stop:     make(chan struct{}),
		stopped:  make(chan struct{}),
		syncKick: make(chan struct{}, 1),
		syncDone: make(chan struct{}),
		files:    map[uint32]File{},
		dirty:    map[uint32]bool{},
	}
	j.done = sync.NewCond(&j.mu)

	return j, nil
}

func (j *Journal) Dir() string {
	return j.dir
}

func (j *Journal) Start() {
	if j.started.CompareAndSwap(false, true) {
		go j.writer()
		go j.syncer()
	}
}

func (j *Journal) SetNextLSN(lsn uint64) {
	j.mu.Lock()
	if lsn > j.nextLSN {
		j.nextLSN = lsn
	}
	j.mu.Unlock()
}

func (j *Journal) NextLSN() uint64 {
	j.mu.Lock()
	defer j.mu.Unlock()

	return j.nextLSN
}

func (j *Journal) Position() Position {
	j.mu.Lock()
	defer j.mu.Unlock()

	return j.positionLocked()
}

func (j *Journal) positionLocked() Position {
	if j.active == 0 {
		return Position{Segment: 1, Offset: segmentHeaderSize}
	}

	return Position{Segment: j.active, Offset: j.activeSize}
}

func (j *Journal) Append(recs []Record) (Ticket, error) {
	if len(recs) == 0 {
		return Ticket{}, nil
	}

	size := batchSize(recs)
	if int64(size) > int64(^uint32(0))-segmentHeaderSize {
		return Ticket{}, fmt.Errorf("journal: batch of %d bytes is too large", size)
	}

	j.mu.Lock()
	if j.closed {
		j.mu.Unlock()
		return Ticket{}, ErrClosed
	}

	if j.active == 0 || (int64(j.activeSize)+int64(size) > j.opts.SegmentSize && j.activeSize > segmentHeaderSize) {
		j.rotateLocked()
	}

	c := j.tailChunkLocked(size)
	before := len(c.data)
	c.data = appendBatch(c.data, recs, j.nextLSN, j.activeSize, j.active)
	written := len(c.data) - before
	j.nextLSN += uint64(len(recs))
	j.activeSize += uint32(written)
	j.appended += uint64(written)
	j.pendingLen += written
	t := Ticket{j: j, at: j.appended}
	urgent := j.opts.Mode != ModeBackground || j.pendingLen >= j.opts.MaxPending
	j.mu.Unlock()

	if urgent {
		j.signal()
	}

	return t, nil
}

func (j *Journal) rotateLocked() {
	j.active++
	j.activeSize = 0
	j.appendBytesLocked(segmentHeader(j.active), true)
}

var chunkBuffers = sync.Pool{New: func() any { return make([]byte, 0, chunkBufferSize) }}

const chunkBufferSize = 64 << 10

func newChunkBuffer(size int) []byte {
	if size > chunkBufferSize {
		return make([]byte, 0, size)
	}

	return chunkBuffers.Get().([]byte)[:0]
}

func releaseChunks(chunks []*chunk) {
	for _, c := range chunks {
		if cap(c.data) == chunkBufferSize {
			chunkBuffers.Put(c.data[:0])
		}
		c.data = nil
	}
}

func (j *Journal) tailChunkLocked(size int) *chunk {
	last := len(j.pending) - 1
	if last >= 0 && j.pending[last].segment == j.active && j.pending[last].offset+uint32(len(j.pending[last].data)) == j.activeSize {
		if c := j.pending[last]; cap(c.data)-len(c.data) >= size {
			return c
		}
	}

	c := &chunk{segment: j.active, offset: j.activeSize, data: newChunkBuffer(size)}
	j.pending = append(j.pending, c)

	return c
}

func (j *Journal) appendBytesLocked(data []byte, fresh bool) {
	last := len(j.pending) - 1
	if !fresh && last >= 0 && j.pending[last].segment == j.active && j.pending[last].offset+uint32(len(j.pending[last].data)) == j.activeSize {
		j.pending[last].data = append(j.pending[last].data, data...)
	} else {
		c := &chunk{segment: j.active, offset: j.activeSize, fresh: fresh, data: append(newChunkBuffer(len(data)), data...)}
		j.pending = append(j.pending, c)
	}

	j.activeSize += uint32(len(data))
	j.appended += uint64(len(data))
	j.pendingLen += len(data)
}

func (j *Journal) signal() {
	select {
	case j.kick <- struct{}{}:
	default:
	}
}

func (t Ticket) Wait() error {
	if t.j == nil || t.j.opts.Mode == ModeBackground {
		return nil
	}

	return t.j.waitWritten(t.at)
}

func (j *Journal) waitWritten(at uint64) error {
	j.mu.Lock()
	defer j.mu.Unlock()

	start := j.attempts
	for j.written < at && !(j.writeErr != nil && j.attempts > start) && !j.closed {
		j.done.Wait()
	}

	if j.written >= at {
		return nil
	}

	if j.writeErr != nil {
		return j.writeErr
	}

	return ErrClosed
}

func (j *Journal) Flush() error {
	j.mu.Lock()
	target := j.appended
	j.mu.Unlock()

	j.signal()
	if err := j.waitWritten(target); err != nil {
		return err
	}

	return j.syncDirty()
}

func (j *Journal) syncDirty() error {
	j.syncMu.Lock()
	defer j.syncMu.Unlock()

	j.filesMu.Lock()
	ids := make([]uint32, 0, len(j.dirty))
	files := make(map[uint32]File, len(j.dirty))
	for id := range j.dirty {
		if f := j.files[id]; f != nil {
			ids = append(ids, id)
			files[id] = f
		}
	}
	j.dirty = map[uint32]bool{}
	j.filesMu.Unlock()

	sort.Slice(ids, func(a, b int) bool { return ids[a] < ids[b] })

	var errs []error
	for _, id := range ids {
		if err := files[id].Sync(); err != nil {
			errs = append(errs, err)
			j.filesMu.Lock()
			j.dirty[id] = true
			j.filesMu.Unlock()
		}
	}

	return errors.Join(errs...)
}

func (j *Journal) Failure() error {
	j.mu.Lock()
	defer j.mu.Unlock()

	return j.writeErr
}

func (j *Journal) syncer() {
	defer close(j.syncDone)

	for {
		select {
		case <-j.syncKick:
			_ = j.syncDirty()
		case <-j.stop:
			return
		}
	}
}

func (j *Journal) noteWritten(n uint64) {
	if j.opts.Mode == ModeSync || j.opts.SyncBytes <= 0 {
		return
	}

	j.unsynced += n
	if j.unsynced < uint64(j.opts.SyncBytes) {
		return
	}

	j.unsynced = 0
	select {
	case j.syncKick <- struct{}{}:
	default:
	}
}

func (j *Journal) writer() {
	defer close(j.stopped)

	var ticker *time.Ticker
	var tick <-chan time.Time
	if j.opts.Mode == ModeBackground && j.opts.FlushInterval > 0 {
		ticker = time.NewTicker(j.opts.FlushInterval)
		tick = ticker.C
		defer ticker.Stop()
	}

	backoff := time.Duration(0)
	for {
		select {
		case <-j.kick:
		case <-tick:
		case <-j.stop:
			j.writeOnce()
			return
		}

		if err := j.writeOnce(); err != nil {
			backoff = min(max(backoff*2, 50*time.Millisecond), 2*time.Second)
			select {
			case <-time.After(backoff):
				j.signal()
			case <-j.stop:
				j.writeOnce()
				return
			}
			continue
		}

		backoff = 0
		j.closeRetired(false)
	}
}

func (j *Journal) writeOnce() error {
	j.mu.Lock()
	if len(j.pending) == 0 {
		j.mu.Unlock()
		return nil
	}

	j.inflight = j.pending
	j.pending = nil
	target := j.appended
	pendingLen := j.pendingLen
	j.pendingLen = 0
	chunks := j.inflight
	j.mu.Unlock()

	err := j.writeChunks(chunks)
	if err == nil && j.opts.Mode == ModeSync {
		err = j.syncDirty()
	}
	if err == nil {
		j.noteWritten(uint64(pendingLen))
	}

	if err != nil && j.opts.OnWriteError != nil {
		j.opts.OnWriteError(err)
	}

	j.mu.Lock()
	j.attempts++
	if err != nil {
		j.pending = append(chunks, j.pending...)
		j.pendingLen += pendingLen
		j.inflight = nil
		j.writeErr = err
		j.done.Broadcast()
		j.mu.Unlock()

		return err
	}

	j.inflight = nil
	j.written = target
	j.writeErr = nil
	last := chunks[len(chunks)-1]
	j.flushed.Store(uint64(last.segment)<<32 | uint64(last.offset+uint32(len(last.data))))
	j.done.Broadcast()
	j.mu.Unlock()

	releaseChunks(chunks)

	return nil
}

func (j *Journal) writeChunks(chunks []*chunk) error {
	for _, c := range chunks {
		f, err := j.fileFor(c.segment, c.fresh && c.offset == 0)
		if err != nil {
			return err
		}

		n, err := f.WriteAt(c.data, int64(c.offset))
		if err != nil {
			return err
		}

		if n != len(c.data) {
			return fmt.Errorf("journal: short write on segment %d (%d of %d bytes)", c.segment, n, len(c.data))
		}

		j.filesMu.Lock()
		j.dirty[c.segment] = true
		j.filesMu.Unlock()
	}

	return nil
}

func (j *Journal) fileFor(segment uint32, create bool) (File, error) {
	j.filesMu.RLock()
	f := j.files[segment]
	if f == nil && !create {
		for _, r := range j.retired {
			if r.id == segment {
				f = r.file
				break
			}
		}
	}
	j.filesMu.RUnlock()
	if f != nil {
		return f, nil
	}

	flag := os.O_RDWR
	if create {
		flag |= os.O_CREATE
	}

	f, err := j.fs.OpenFile(filepath.Join(j.dir, SegmentName(segment)), flag, 0o600)
	if err != nil {
		return nil, err
	}

	j.filesMu.Lock()
	if existing := j.files[segment]; existing != nil {
		j.filesMu.Unlock()
		_ = f.Close()
		return existing, nil
	}
	j.files[segment] = f
	j.filesMu.Unlock()

	if create {
		if err := j.fs.SyncDir(j.dir); err != nil {
			return nil, err
		}
		durable.Preallocate(f, j.dir, j.opts.SegmentSize)

		j.filesMu.RLock()
		previous := j.files[segment-1]
		j.filesMu.RUnlock()
		if previous != nil {
			durable.TrimPreallocated(previous)
		}
	}

	return f, nil
}

func (j *Journal) ReadValue(loc Location) ([]byte, error) {
	out := make([]byte, loc.Length)
	if loc.Length == 0 {
		return out, nil
	}

	end := uint64(loc.Segment)<<32 | uint64(loc.Offset+loc.Length)
	if end > j.flushed.Load() {
		j.mu.Lock()
		found := copyFromChunks(out, loc, j.pending) || copyFromChunks(out, loc, j.inflight)
		j.mu.Unlock()
		if found {
			return out, nil
		}
	}

	f, err := j.fileFor(loc.Segment, false)
	if err != nil {
		return nil, err
	}

	n, err := f.ReadAt(out, int64(loc.Offset))
	if n == len(out) {
		return out, nil
	}

	if err == nil {
		err = fmt.Errorf("journal: short read on segment %d", loc.Segment)
	}

	return nil, err
}

type ReadWindow struct {
	buf     []byte
	segment uint32
	start   uint32
	size    uint32
	pace    func(time.Duration)
}

func NewReadWindow(size int, pace func(time.Duration)) *ReadWindow {
	return &ReadWindow{buf: make([]byte, size), pace: pace}
}

func (j *Journal) ReadThrough(loc Location, w *ReadWindow) ([]byte, error) {
	end := uint64(loc.Segment)<<32 | uint64(loc.Offset+loc.Length)
	if loc.Length == 0 || int(loc.Length) > len(w.buf) || end > j.flushed.Load() {
		return j.ReadValue(loc)
	}

	if w.size == 0 || w.segment != loc.Segment || loc.Offset < w.start || loc.Offset+loc.Length > w.start+w.size {
		f, err := j.fileFor(loc.Segment, false)
		if err != nil {
			return nil, err
		}

		start := time.Now()
		n, err := f.ReadAt(w.buf, int64(loc.Offset))
		if w.pace != nil {
			w.pace(time.Since(start))
		}
		if n < int(loc.Length) {
			w.size = 0
			if err == nil {
				err = fmt.Errorf("journal: short read on segment %d", loc.Segment)
			}
			return nil, err
		}

		w.segment, w.start, w.size = loc.Segment, loc.Offset, uint32(n)
	}

	return w.buf[loc.Offset-w.start : loc.Offset-w.start+loc.Length], nil
}

func copyFromChunks(out []byte, loc Location, chunks []*chunk) bool {
	for _, c := range chunks {
		if c.segment != loc.Segment || loc.Offset < c.offset || loc.Offset+loc.Length > c.offset+uint32(len(c.data)) {
			continue
		}

		copy(out, c.data[loc.Offset-c.offset:])

		return true
	}

	return false
}

type SegmentInfo struct {
	ID   uint32
	Size int64
}

func (j *Journal) BytesFrom(from Position) int64 {
	segments, err := j.Segments()
	if err != nil {
		return 0
	}

	var n int64
	for _, seg := range segments {
		if seg.ID < from.Segment {
			continue
		}

		n += seg.Size
		if seg.ID == from.Segment {
			n -= int64(from.Offset)
		}
	}

	return max(n, 0)
}

func (j *Journal) Segments() ([]SegmentInfo, error) {
	entries, err := j.fs.ReadDir(j.dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}

		return nil, err
	}

	out := []SegmentInfo{}
	for _, e := range entries {
		name := e.Name()
		if !strings.HasSuffix(name, segmentSuffix) {
			continue
		}

		id, err := strconv.ParseUint(strings.TrimSuffix(name, segmentSuffix), 10, 32)
		if err != nil {
			continue
		}

		info, err := e.Info()
		if err != nil {
			continue
		}

		out = append(out, SegmentInfo{ID: uint32(id), Size: info.Size()})
	}

	sort.Slice(out, func(a, b int) bool { return out[a].ID < out[b].ID })

	return out, nil
}

func (j *Journal) ActiveSegment() uint32 {
	j.mu.Lock()
	defer j.mu.Unlock()

	return j.active
}

type ReplayStats struct {
	SetAside  []uint32
	Damaged   []uint32
	Batches   int
	Records   int
	Truncated int64
	LastLSN   uint64
}

func (j *Journal) Replay(from Position, apply func(recs []Record) error) (ReplayStats, error) {
	stats := ReplayStats{}
	segments, err := j.Segments()
	if err != nil {
		return stats, err
	}

	end := Position{}
	for idx, seg := range segments {
		if seg.ID < from.Segment {
			continue
		}

		last := idx == len(segments)-1
		start := uint32(segmentHeaderSize)
		if seg.ID == from.Segment && from.Offset > start {
			start = from.Offset
		}

		stop, truncated, torn, err := j.replaySegment(seg.ID, start, apply, &stats)
		if err != nil {
			return stats, err
		}

		stats.Truncated += truncated
		end = Position{Segment: seg.ID, Offset: stop}

		if torn && !last {
			for _, later := range segments[idx+1:] {
				stats.SetAside = append(stats.SetAside, later.ID)
				if err := j.setAside(later.ID); err != nil {
					return stats, err
				}
			}
			break
		}
	}

	j.mu.Lock()
	if end.Segment != 0 {
		j.active = end.Segment
		j.activeSize = end.Offset
		j.written = 0
		j.appended = 0
		j.flushed.Store(uint64(end.Segment)<<32 | uint64(end.Offset))
	}
	if stats.LastLSN+1 > j.nextLSN {
		j.nextLSN = stats.LastLSN + 1
	}
	j.mu.Unlock()

	return stats, nil
}

func validBatchAfter(data []byte, from int, segment uint32) bool {
	var magic [4]byte
	binary.LittleEndian.PutUint32(magic[:], batchMagic)

	for from < len(data) {
		i := bytes.Index(data[from:], magic[:])
		if i < 0 {
			return false
		}

		from += i
		if _, _, err := decodeBatch(data[from:], segment, uint32(from)); err == nil {
			return true
		}
		from++
	}

	return false
}

func (j *Journal) keepDamagedTail(id uint32, tail []byte) error {
	path := filepath.Join(j.dir, SegmentName(id)+".tail"+damagedSuffix)

	return durable.WriteFileAtomic(j.fs, path, func(w io.Writer) error {
		_, err := w.Write(tail)
		return err
	})
}

func (j *Journal) setAside(id uint32) error {
	path := filepath.Join(j.dir, SegmentName(id))

	return j.fs.Rename(path, path+damagedSuffix)
}

func (j *Journal) replaySegment(id uint32, start uint32, apply func([]Record) error, stats *ReplayStats) (uint32, int64, bool, error) {
	path := filepath.Join(j.dir, SegmentName(id))
	f, err := j.fs.OpenFile(path, os.O_RDWR, 0o600)
	if err != nil {
		return 0, 0, false, err
	}

	info, err := f.Stat()
	if err != nil {
		_ = f.Close()
		return 0, 0, false, err
	}

	data := make([]byte, info.Size())
	if _, err := f.ReadAt(data, 0); err != nil && int64(len(data)) != info.Size() {
		_ = f.Close()
		return 0, 0, false, err
	}

	if len(data) < segmentHeaderSize || checkSegmentHeader(data, id) != nil {
		if err := f.Truncate(0); err != nil {
			_ = f.Close()
			return 0, 0, false, err
		}

		if _, err := f.WriteAt(segmentHeader(id), 0); err != nil {
			_ = f.Close()
			return 0, 0, false, err
		}

		j.adoptFile(id, f)

		return segmentHeaderSize, int64(len(data)), true, nil
	}

	pos := int(start)
	for pos < len(data) {
		recs, n, err := decodeBatch(data[pos:], id, uint32(pos))
		if err != nil {
			if validBatchAfter(data, pos+1, id) {
				if err := j.keepDamagedTail(id, data[pos:]); err != nil {
					_ = f.Close()
					return 0, 0, false, err
				}
				stats.Damaged = append(stats.Damaged, id)
			}

			truncated := int64(len(data) - pos)
			if err := f.Truncate(int64(pos)); err != nil {
				_ = f.Close()
				return 0, 0, false, err
			}

			j.adoptFile(id, f)

			return uint32(pos), truncated, true, nil
		}

		for i := range recs {
			if recs[i].LSN > stats.LastLSN {
				stats.LastLSN = recs[i].LSN
			}
		}

		if err := apply(recs); err != nil {
			_ = f.Close()
			return 0, 0, false, err
		}

		stats.Batches++
		stats.Records += len(recs)
		pos += n
	}

	j.adoptFile(id, f)

	return uint32(len(data)), 0, false, nil
}

func (j *Journal) adoptFile(id uint32, f File) {
	j.filesMu.Lock()
	if existing := j.files[id]; existing != nil {
		_ = existing.Close()
	}
	j.files[id] = f
	j.filesMu.Unlock()
}

func (j *Journal) ScanSegment(id uint32, fn func(recs []Record) error) error {
	f, err := j.fileFor(id, false)
	if err != nil {
		return err
	}

	info, err := f.Stat()
	if err != nil {
		return err
	}

	data := make([]byte, info.Size())
	if _, err := f.ReadAt(data, 0); err != nil && int64(len(data)) != info.Size() {
		return err
	}

	if err := checkSegmentHeader(data, id); err != nil {
		return err
	}

	pos := segmentHeaderSize
	for pos < len(data) {
		recs, n, err := decodeBatch(data[pos:], id, uint32(pos))
		if err != nil {
			return fmt.Errorf("segment %d offset %d: %w", id, pos, err)
		}

		if err := fn(recs); err != nil {
			return err
		}

		pos += n
	}

	return nil
}

func (j *Journal) Retire(id uint32) error {
	j.mu.Lock()
	active := j.active
	j.mu.Unlock()
	if id >= active {
		return fmt.Errorf("journal: segment %d is still active", id)
	}

	path := filepath.Join(j.dir, SegmentName(id))

	j.filesMu.Lock()
	f := j.files[id]
	if f == nil {
		if opened, err := j.fs.OpenFile(path, os.O_RDONLY, 0o600); err == nil {
			f = opened
		}
	}
	delete(j.files, id)
	delete(j.dirty, id)
	if f != nil {
		j.retired = append(j.retired, retiredFile{id: id, file: f, since: time.Now()})
	}
	j.filesMu.Unlock()

	return j.fs.Remove(path)
}

func (j *Journal) Hold() {
	j.holds.Add(1)
}

func (j *Journal) Release() {
	if j.holds.Add(-1) == 0 {
		j.closeRetired(false)
	}
}

func (j *Journal) CloseRetired() {
	j.closeRetired(false)
}

func (j *Journal) closeRetired(all bool) {
	if !all && j.holds.Load() > 0 {
		return
	}

	j.filesMu.Lock()
	kept := j.retired[:0]
	for _, r := range j.retired {
		if all || time.Since(r.since) > retireGrace {
			_ = r.file.Close()
			continue
		}

		kept = append(kept, r)
	}
	j.retired = kept
	j.filesMu.Unlock()
}

func (j *Journal) PendingBytes() int {
	j.mu.Lock()
	defer j.mu.Unlock()

	return j.pendingLen
}

func (j *Journal) Close() error {
	j.mu.Lock()
	if j.closed {
		j.mu.Unlock()
		return nil
	}
	j.mu.Unlock()

	j.stopWriter()

	j.mu.Lock()
	j.closed = true
	err := j.writeErr
	if len(j.pending) > 0 && err == nil {
		err = errors.New("journal: unwritten data at close")
	}
	j.done.Broadcast()
	j.mu.Unlock()

	if syncErr := j.syncDirty(); syncErr != nil && err == nil {
		err = syncErr
	}

	j.filesMu.Lock()
	if last := j.files[j.active]; last != nil {
		durable.TrimPreallocated(last)
	}
	for id, f := range j.files {
		_ = f.Close()
		delete(j.files, id)
	}
	j.filesMu.Unlock()
	j.closeRetired(true)

	return err
}

func (j *Journal) stopWriter() {
	if j.started.Load() {
		close(j.stop)
		<-j.stopped
		<-j.syncDone
	}
}

func (j *Journal) Abandon() {
	j.mu.Lock()
	if j.closed {
		j.mu.Unlock()
		return
	}

	j.closed = true
	j.pending = nil
	j.pendingLen = 0
	j.done.Broadcast()
	j.mu.Unlock()

	j.stopWriter()

	j.filesMu.Lock()
	for id, f := range j.files {
		_ = f.Close()
		delete(j.files, id)
	}
	j.filesMu.Unlock()
	j.closeRetired(true)
}
