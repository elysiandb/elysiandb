//go:build linux

package durable

import (
	"os"
	"syscall"
)

const fallocKeepSize = 1

func preallocate(f *os.File, size int64) {
	_ = syscall.Fallocate(int(f.Fd()), fallocKeepSize, 0, size)
}
