//go:build !linux

package durable

import "os"

func preallocate(*os.File, int64) {}
