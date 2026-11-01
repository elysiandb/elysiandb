package metrics

import (
	"math"
	"sync/atomic"
	"time"
)

var bucketBoundsMs = [...]float64{0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000}

const numBuckets = len(bucketBoundsMs) + 1

type Histogram struct {
	counts [numBuckets]atomic.Uint64
	sum    atomic.Uint64
}

type hist [numBuckets]uint64

func bucketFor(ms float64) int {
	for i, bound := range bucketBoundsMs {
		if ms <= bound {
			return i
		}
	}

	return numBuckets - 1
}

func (h *Histogram) Observe(d time.Duration) {
	h.counts[bucketFor(float64(d)/float64(time.Millisecond))].Add(1)
	if d > 0 {
		h.sum.Add(uint64(d))
	}
}

func (h *Histogram) sumSeconds() float64 {
	return float64(h.sum.Load()) / float64(time.Second)
}

func (h *Histogram) snapshot() hist {
	var out hist
	for i := range h.counts {
		out[i] = h.counts[i].Load()
	}

	return out
}

func (h hist) sub(o hist) hist {
	var out hist
	for i := range h {
		if h[i] >= o[i] {
			out[i] = h[i] - o[i]
		}
	}

	return out
}

func (h hist) add(o hist) hist {
	var out hist
	for i := range h {
		out[i] = h[i] + o[i]
	}

	return out
}

func (h hist) total() uint64 {
	var t uint64
	for _, c := range h {
		t += c
	}

	return t
}

func (h hist) quantile(q float64) float64 {
	total := h.total()
	if total == 0 {
		return math.NaN()
	}

	rank := q * float64(total)
	var seen float64
	for i, c := range h {
		if c == 0 {
			continue
		}

		if seen+float64(c) >= rank {
			lower := 0.0
			if i > 0 {
				lower = bucketBoundsMs[i-1]
			}

			if i == numBuckets-1 {
				return lower
			}

			upper := bucketBoundsMs[i]
			return lower + (upper-lower)*(rank-seen)/float64(c)
		}

		seen += float64(c)
	}

	return bucketBoundsMs[len(bucketBoundsMs)-1]
}

func floatHistQuantile(counts []uint64, buckets []float64, q float64) float64 {
	var total uint64
	for _, c := range counts {
		total += c
	}

	if total == 0 {
		return math.NaN()
	}

	rank := q * float64(total)
	var seen float64
	for i, c := range counts {
		seen += float64(c)
		if seen >= rank {
			upper := buckets[i+1]
			if math.IsInf(upper, 1) {
				upper = buckets[i]
			}

			return upper
		}
	}

	return buckets[len(buckets)-1]
}
