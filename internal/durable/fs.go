package durable

import (
	"bufio"
	"io"
	"os"
	"path/filepath"
)

type File interface {
	io.ReaderAt
	io.WriterAt
	Sync() error
	Truncate(size int64) error
	Close() error
	Stat() (os.FileInfo, error)
}

type FS interface {
	OpenFile(name string, flag int, perm os.FileMode) (File, error)
	Remove(name string) error
	Rename(from, to string) error
	ReadDir(name string) ([]os.DirEntry, error)
	MkdirAll(name string, perm os.FileMode) error
	SyncDir(name string) error
}

type osFS struct{}

func (osFS) OpenFile(name string, flag int, perm os.FileMode) (File, error) {
	return os.OpenFile(name, flag, perm)
}

func (osFS) Remove(name string) error {
	return os.Remove(name)
}

func (osFS) Rename(from, to string) error {
	return os.Rename(from, to)
}

func (osFS) ReadDir(name string) ([]os.DirEntry, error) {
	return os.ReadDir(name)
}

func (osFS) MkdirAll(name string, perm os.FileMode) error {
	return os.MkdirAll(name, perm)
}

func (osFS) SyncDir(name string) error {
	d, err := os.Open(name)
	if err != nil {
		return err
	}

	defer d.Close()

	_ = d.Sync()

	return nil
}

var OS FS = osFS{}

func Or(fs FS) FS {
	if fs == nil {
		return OS
	}

	return fs
}

type offsetWriter struct {
	f   File
	off int64
}

func (w *offsetWriter) Write(p []byte) (int, error) {
	n, err := w.f.WriteAt(p, w.off)
	w.off += int64(n)

	return n, err
}

func WriteFileAtomic(fs FS, path string, write func(w io.Writer) error) error {
	fs = Or(fs)
	tmp := path + ".tmp"
	f, err := fs.OpenFile(tmp, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}

	bw := bufio.NewWriterSize(&offsetWriter{f: f}, 1<<20)
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
		_ = fs.Remove(tmp)
		return err
	}

	if err := fs.Rename(tmp, path); err != nil {
		_ = fs.Remove(tmp)
		return err
	}

	return fs.SyncDir(filepath.Dir(path))
}

func ReadFile(fs FS, path string) ([]byte, error) {
	f, err := Or(fs).OpenFile(path, os.O_RDONLY, 0)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	info, err := f.Stat()
	if err != nil {
		return nil, err
	}

	data := make([]byte, info.Size())
	if _, err := f.ReadAt(data, 0); err != nil && err != io.EOF {
		return nil, err
	}

	return data, nil
}
