//go:build !linux

package durable

func RunGently(fn func()) {
	fn()
}
