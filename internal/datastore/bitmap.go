package datastore

import (
	"encoding/binary"
	"math/bits"

	"github.com/taymour/elysiandb/internal/pagetree"
)

func bitmapKey(value string, chunk uint32) string {
	var b [4]byte
	binary.BigEndian.PutUint32(b[:], chunk)

	return value + string(b[:])
}

func bitmapValue(key string) string {
	return key[:len(key)-4]
}

func bitmapChunk(key string) uint32 {
	return binary.BigEndian.Uint32([]byte(key[len(key)-4:]))
}

func postingCount(c []byte) int {
	if len(c) == 0 {
		return 0
	}

	if c[0] != 1 {
		return (len(c) - 1) / 2
	}

	n := 0
	for i := 1; i+8 <= len(c); i += 8 {
		n += bits.OnesCount64(binary.LittleEndian.Uint64(c[i:]))
	}

	return n
}

func updateBitmap(tree pagetree.Tree, seq uint32, removed, added []string) pagetree.Tree {
	chunk := seq >> postingChunkBits
	off := uint16(seq & (postingChunk - 1))

	for _, v := range removed {
		key := bitmapKey(v, chunk)
		cur, ok := tree.Get(key)
		if !ok {
			continue
		}

		next := postingRemove(cur, off)
		if next == nil {
			tree, _ = tree.Delete(key)
			continue
		}

		tree, _ = tree.Update(key, func([]byte) []byte { return next })
	}

	for _, v := range added {
		tree = tree.Upsert(bitmapKey(v, chunk), func(c []byte, found bool) []byte {
			if !found {
				return postingAdd(nil, off)
			}
			return postingAdd(c, off)
		})
	}

	return tree
}

func (v *IndexView) bitmap() bool {
	return v.idx.spec.Kind == IndexBitmap
}

func (v *IndexView) bitmapLen() int {
	n := 0
	v.idx.tree.AscendAt(0, func(_ string, c []byte) bool {
		n += postingCount(c)
		return true
	})

	return n
}

func (v *IndexView) bitmapRank(prefix []byte) int {
	n := 0
	p := string(prefix)
	v.idx.tree.AscendAt(0, func(k string, c []byte) bool {
		if k >= p {
			return false
		}

		value := bitmapValue(k)
		if value+"\x00\x00\x00\x00" >= p {
			postingEach(c, bitmapChunk(k)<<postingChunkBits, func(seq uint32) {
				if value+seqKey(seq) < p {
					n++
				}
			})
			return true
		}

		n += postingCount(c)
		return true
	})

	return n
}

func (v *IndexView) bitmapScan(lo, hi []byte, fn func(key string, seq uint32) bool) {
	l := string(lo)
	start := ""
	if len(l) > 4 {
		start = l[:len(l)-4]
	}

	v.idx.tree.AscendFrom(start, func(k string, c []byte) bool {
		value := bitmapValue(k)
		if hi != nil && value+"\x00\x00\x00\x00" >= string(hi) {
			return false
		}

		more := true
		postingEach(c, bitmapChunk(k)<<postingChunkBits, func(seq uint32) {
			if !more {
				return
			}

			key := value + seqKey(seq)
			if key < l {
				return
			}

			if hi != nil && key >= string(hi) {
				more = false
				return
			}

			more = fn(key, seq)
		})

		return more
	})
}

func (v *IndexView) Collect(lo, hi []byte, set *Bitset, keep func(key string) bool) {
	if !v.bitmap() {
		v.ScanRange(lo, hi, func(key string, seq uint32) bool {
			if keep == nil || keep(key) {
				set.Set(seq)
			}
			return true
		})
		return
	}

	l := string(lo)
	start := ""
	if len(l) > 4 {
		start = l[:len(l)-4]
	}

	var lastValue string
	seen, keepValue := false, false
	v.idx.tree.AscendFrom(start, func(k string, c []byte) bool {
		value := bitmapValue(k)
		if hi != nil && value+"\x00\x00\x00\x00" >= string(hi) {
			return false
		}

		whole := value >= l && (hi == nil || value+"\xff\xff\xff\xff" < string(hi))
		if !whole {
			base := bitmapChunk(k) << postingChunkBits
			postingEach(c, base, func(seq uint32) {
				key := value + seqKey(seq)
				if key >= l && (hi == nil || key < string(hi)) && (keep == nil || keep(key)) {
					set.Set(seq)
				}
			})
			return true
		}

		if keep != nil {
			if !seen || value != lastValue {
				seen, lastValue = true, value
				keepValue = keep(value + "\x00\x00\x00\x00")
			}
			if !keepValue {
				return true
			}
		}

		set.addContainer(bitmapChunk(k), c)
		return true
	})
}

func (v *IndexView) DistinctValues(limit int) int {
	n := 0
	last := ""
	v.idx.tree.AscendAt(0, func(k string, _ []byte) bool {
		value := bitmapValue(k)
		if n > 0 && value == last {
			return true
		}
		last = value
		n++
		return n <= limit
	})

	return n
}
