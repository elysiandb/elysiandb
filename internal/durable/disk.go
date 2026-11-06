package durable

import (
	"sync/atomic"
	"syscall"
	"time"
)

type IOStats struct {
	ReadBytes  int64
	WriteBytes int64
	Syncs      int64
	SyncNanos  int64
}

var (
	ioReadBytes  atomic.Int64
	ioWriteBytes atomic.Int64
	syncs        atomic.Int64
	syncNanos    atomic.Int64
)

func Stats() IOStats {
	return IOStats{ReadBytes: ioReadBytes.Load(), WriteBytes: ioWriteBytes.Load(), Syncs: syncs.Load(), SyncNanos: syncNanos.Load()}
}

func noteSync(start time.Time) {
	syncs.Add(1)
	syncNanos.Add(int64(time.Since(start)))
}

func FreeBytes(dir string) (int64, error) {
	var st syscall.Statfs_t
	if err := syscall.Statfs(dir, &st); err != nil {
		return 0, err
	}

	return int64(st.Bavail) * int64(st.Bsize), nil
}

const preallocateFreeFactor = 8

func Preallocate(f File, dir string, size int64) {
	c, ok := f.(countedFile)
	if !ok || size <= 0 {
		return
	}

	if free, err := FreeBytes(dir); err != nil || free < preallocateFreeFactor*size {
		return
	}

	preallocate(c.File, size)
}

func TrimPreallocated(f File) {
	c, ok := f.(countedFile)
	if !ok {
		return
	}

	if st, err := c.Stat(); err == nil {
		_ = c.Truncate(st.Size())
	}
}
