package journal

import (
	"io"
	"os"
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
	ReadDir(name string) ([]os.DirEntry, error)
	MkdirAll(name string, perm os.FileMode) error
}

type osFS struct{}

func (osFS) OpenFile(name string, flag int, perm os.FileMode) (File, error) {
	return os.OpenFile(name, flag, perm)
}

func (osFS) Remove(name string) error {
	return os.Remove(name)
}

func (osFS) ReadDir(name string) ([]os.DirEntry, error) {
	return os.ReadDir(name)
}

func (osFS) MkdirAll(name string, perm os.FileMode) error {
	return os.MkdirAll(name, perm)
}

var OSFS FS = osFS{}
