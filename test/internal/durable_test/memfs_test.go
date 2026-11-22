package durable_test

import (
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/taymour/elysiandb/internal/durable"
)

var errCrashed = errors.New("memfs: the machine is down")

type memNode struct {
	data   []byte
	synced []byte
}

type memFS struct {
	mu      sync.Mutex
	names   map[string]*memNode
	durable map[string]*memNode
	dirs    map[string]bool
	ops     int
	crashAt int
	crashed bool
	down    chan struct{}
	quota   int64
	used    int64
}

func newMemFS() *memFS {
	return &memFS{names: map[string]*memNode{}, durable: map[string]*memNode{}, dirs: map[string]bool{}, down: make(chan struct{}), quota: -1}
}

func (m *memFS) step() error {
	if m.crashed {
		return errCrashed
	}

	m.ops++
	if m.crashAt > 0 && m.ops >= m.crashAt {
		m.crashed = true
		close(m.down)
		return errCrashed
	}

	return nil
}

func (m *memFS) Ops() int {
	m.mu.Lock()
	defer m.mu.Unlock()

	return m.ops
}

func (m *memFS) SetQuota(n int64) {
	m.mu.Lock()
	m.quota = n
	m.mu.Unlock()
}

func (m *memFS) Usage() int64 {
	m.mu.Lock()
	defer m.mu.Unlock()

	return m.used
}

func (m *memFS) afterPowerLoss() *memFS {
	m.mu.Lock()
	defer m.mu.Unlock()

	out := newMemFS()
	for name, n := range m.durable {
		c := &memNode{data: append([]byte(nil), n.synced...), synced: append([]byte(nil), n.synced...)}
		out.names[name] = c
		out.durable[name] = c
	}
	for d := range m.dirs {
		out.dirs[d] = true
	}

	return out
}

func (m *memFS) afterProcessCrash() *memFS {
	m.mu.Lock()
	defer m.mu.Unlock()

	out := newMemFS()
	for name, n := range m.names {
		c := &memNode{data: append([]byte(nil), n.data...), synced: append([]byte(nil), n.data...)}
		out.names[name] = c
		out.durable[name] = c
	}
	for d := range m.dirs {
		out.dirs[d] = true
	}

	return out
}

func (m *memFS) mutate(name string, fn func(n *memNode)) {
	m.mu.Lock()
	defer m.mu.Unlock()

	if n := m.names[name]; n != nil {
		fn(n)
	}
}

func (m *memFS) list(prefix string) []string {
	m.mu.Lock()
	defer m.mu.Unlock()

	var out []string
	for name := range m.names {
		if strings.HasPrefix(name, prefix) {
			out = append(out, name)
		}
	}
	sort.Strings(out)

	return out
}

func (m *memFS) OpenFile(name string, flag int, perm os.FileMode) (durable.File, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	n, exists := m.names[name]
	if flag&(os.O_CREATE|os.O_TRUNC) != 0 {
		if err := m.step(); err != nil {
			return nil, err
		}
	}

	switch {
	case exists && flag&os.O_CREATE != 0 && flag&os.O_EXCL != 0:
		return nil, fs.ErrExist
	case !exists && flag&os.O_CREATE == 0:
		return nil, &fs.PathError{Op: "open", Path: name, Err: fs.ErrNotExist}
	case !exists:
		n = &memNode{}
		m.names[name] = n
	}

	if flag&os.O_TRUNC != 0 {
		m.used -= int64(len(n.data))
		n.data = nil
	}

	return &memFile{fs: m, name: name, node: n}, nil
}

func (m *memFS) Remove(name string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if err := m.step(); err != nil {
		return err
	}

	n, ok := m.names[name]
	if !ok {
		return &fs.PathError{Op: "remove", Path: name, Err: fs.ErrNotExist}
	}

	m.used -= int64(len(n.data))
	delete(m.names, name)

	return nil
}

func (m *memFS) Rename(from, to string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if err := m.step(); err != nil {
		return err
	}

	n, ok := m.names[from]
	if !ok {
		return &fs.PathError{Op: "rename", Path: from, Err: fs.ErrNotExist}
	}

	if old, ok := m.names[to]; ok {
		m.used -= int64(len(old.data))
	}

	m.names[to] = n
	delete(m.names, from)

	return nil
}

type memInfo struct {
	name string
	size int64
	dir  bool
}

func (i memInfo) Name() string { return i.name }
func (i memInfo) Size() int64  { return i.size }
func (i memInfo) Mode() fs.FileMode {
	if i.dir {
		return fs.ModeDir | 0o755
	}
	return 0o600
}
func (i memInfo) ModTime() time.Time { return time.Time{} }
func (i memInfo) IsDir() bool        { return i.dir }
func (i memInfo) Sys() any           { return nil }

func (m *memFS) ReadDir(name string) ([]os.DirEntry, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	if !m.dirs[name] {
		return nil, &fs.PathError{Op: "readdir", Path: name, Err: fs.ErrNotExist}
	}

	var out []os.DirEntry
	for path, n := range m.names {
		if filepath.Dir(path) == name {
			out = append(out, fs.FileInfoToDirEntry(memInfo{name: filepath.Base(path), size: int64(len(n.data))}))
		}
	}
	for d := range m.dirs {
		if filepath.Dir(d) == name && d != name {
			out = append(out, fs.FileInfoToDirEntry(memInfo{name: filepath.Base(d), dir: true}))
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name() < out[j].Name() })

	return out, nil
}

func (m *memFS) MkdirAll(name string, perm os.FileMode) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if m.crashed {
		return errCrashed
	}

	for d := name; d != "/" && d != "." && !m.dirs[d]; d = filepath.Dir(d) {
		m.dirs[d] = true
	}

	return nil
}

func (m *memFS) SyncDir(name string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if err := m.step(); err != nil {
		return err
	}

	for path := range m.durable {
		if filepath.Dir(path) == name {
			if _, ok := m.names[path]; !ok {
				delete(m.durable, path)
			}
		}
	}

	for path, n := range m.names {
		if filepath.Dir(path) == name {
			m.durable[path] = n
		}
	}

	return nil
}

type memFile struct {
	fs   *memFS
	name string
	node *memNode
}

func (f *memFile) ReadAt(p []byte, off int64) (int, error) {
	f.fs.mu.Lock()
	defer f.fs.mu.Unlock()

	if f.fs.crashed {
		return 0, errCrashed
	}

	if off >= int64(len(f.node.data)) {
		return 0, io.EOF
	}

	n := copy(p, f.node.data[off:])
	if n < len(p) {
		return n, io.EOF
	}

	return n, nil
}

func (f *memFile) WriteAt(p []byte, off int64) (int, error) {
	f.fs.mu.Lock()
	defer f.fs.mu.Unlock()

	if err := f.fs.step(); err != nil {
		return 0, err
	}

	end := off + int64(len(p))
	grow := max(0, end-int64(len(f.node.data)))
	if f.fs.quota >= 0 && f.fs.used+grow > f.fs.quota {
		return 0, syscall.ENOSPC
	}

	if grow > 0 {
		f.node.data = append(f.node.data, make([]byte, grow)...)
		f.fs.used += grow
	}
	copy(f.node.data[off:], p)

	return len(p), nil
}

func (f *memFile) Sync() error {
	f.fs.mu.Lock()
	defer f.fs.mu.Unlock()

	if err := f.fs.step(); err != nil {
		return err
	}

	f.node.synced = append(f.node.synced[:0], f.node.data...)

	return nil
}

func (f *memFile) Truncate(size int64) error {
	f.fs.mu.Lock()
	defer f.fs.mu.Unlock()

	if err := f.fs.step(); err != nil {
		return err
	}

	if size < int64(len(f.node.data)) {
		f.fs.used -= int64(len(f.node.data)) - size
		f.node.data = f.node.data[:size]
	}

	return nil
}

func (f *memFile) Close() error {
	return nil
}

func (f *memFile) Stat() (os.FileInfo, error) {
	f.fs.mu.Lock()
	defer f.fs.mu.Unlock()

	return memInfo{name: filepath.Base(f.name), size: int64(len(f.node.data))}, nil
}
