//go:build !unix

package metrics

func processCPUSeconds() float64 {
	return -1
}
