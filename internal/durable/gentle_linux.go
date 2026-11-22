//go:build linux

package durable

import (
	"runtime"
	"syscall"
)

const (
	ioprioWhoProcess = 1
	ioprioClassBE    = 2
	ioprioClassShift = 13
	ioprioLowest     = 7
	gentleNice       = 10
)

func RunGently(fn func()) {
	done := make(chan struct{})
	go func() {
		defer close(done)
		runtime.LockOSThread()
		_, _, _ = syscall.Syscall(syscall.SYS_IOPRIO_SET, ioprioWhoProcess, 0, ioprioClassBE<<ioprioClassShift|ioprioLowest)
		_ = syscall.Setpriority(syscall.PRIO_PROCESS, 0, gentleNice)
		fn()
	}()
	<-done
}
