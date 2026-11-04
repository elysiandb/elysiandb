package datastore

import (
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"github.com/cockroachdb/pebble/v2/vfs"
)

const walCategory = "pebble-wal"

type walFS struct {
	vfs.FS
	deferSync bool

	mu    sync.Mutex
	files map[*walFile]struct{}
}

func newWALFS(deferSync bool) *walFS {
	return &walFS{FS: vfs.Default, deferSync: deferSync, files: map[*walFile]struct{}{}}
}

func isWAL(name string, category vfs.DiskWriteCategory) bool {
	return category == walCategory || strings.HasSuffix(name, ".log")
}

func ownerOnly(name string) {
	_ = os.Chmod(name, 0o600)
}

func ownerOnlyTree(dir string) {
	_ = filepath.WalkDir(dir, func(path string, d fs.DirEntry, err error) error {
		if err == nil && d.Type().IsRegular() {
			ownerOnly(path)
		}
		return nil
	})
}

func (fs *walFS) track(f vfs.File, name string, category vfs.DiskWriteCategory) vfs.File {
	ownerOnly(name)
	if !fs.deferSync || !isWAL(name, category) {
		return f
	}

	w := &walFile{File: f, fs: fs}
	fs.mu.Lock()
	fs.files[w] = struct{}{}
	fs.mu.Unlock()

	return w
}

func (fs *walFS) Create(name string, category vfs.DiskWriteCategory) (vfs.File, error) {
	f, err := fs.FS.Create(name, category)
	if err != nil {
		return nil, err
	}

	return fs.track(f, name, category), nil
}

func (fs *walFS) ReuseForWrite(oldname, newname string, category vfs.DiskWriteCategory) (vfs.File, error) {
	f, err := fs.FS.ReuseForWrite(oldname, newname, category)
	if err != nil {
		return nil, err
	}

	return fs.track(f, newname, category), nil
}

func (fs *walFS) OpenReadWrite(name string, category vfs.DiskWriteCategory, opts ...vfs.OpenOption) (vfs.File, error) {
	f, err := fs.FS.OpenReadWrite(name, category, opts...)
	if err != nil {
		return nil, err
	}

	ownerOnly(name)

	return f, nil
}

func (fs *walFS) Rename(oldname, newname string) error {
	if err := fs.FS.Rename(oldname, newname); err != nil {
		return err
	}

	ownerOnly(newname)

	return nil
}

func (fs *walFS) syncAll() error {
	if !fs.deferSync {
		return nil
	}

	fs.mu.Lock()
	files := make([]*walFile, 0, len(fs.files))
	for f := range fs.files {
		files = append(files, f)
	}
	fs.mu.Unlock()

	var first error
	for _, f := range files {
		if err := f.syncNow(); err != nil && first == nil {
			first = err
		}
	}

	return first
}

type walFile struct {
	vfs.File
	fs *walFS

	mu     sync.Mutex
	closed bool
}

func (f *walFile) Sync() error {
	return nil
}

func (f *walFile) SyncData() error {
	return nil
}

func (f *walFile) SyncTo(int64) (bool, error) {
	return false, nil
}

func (f *walFile) syncNow() error {
	f.mu.Lock()
	defer f.mu.Unlock()

	if f.closed {
		return nil
	}

	return f.File.Sync()
}

func (f *walFile) Close() error {
	f.mu.Lock()
	err := f.File.Sync()
	f.closed = true
	f.mu.Unlock()

	f.fs.mu.Lock()
	delete(f.fs.files, f)
	f.fs.mu.Unlock()

	if closeErr := f.File.Close(); err == nil {
		err = closeErr
	}

	return err
}
