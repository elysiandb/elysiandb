package api_storage

import (
	"encoding/binary"
	"math"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/docpath"
)

const evalMinPending = 64

var zeroTime = time.Time{}

func kindCount(view *datastore.IndexView, kind byte) int {
	return view.CountKind(kind)
}

func pendingCount(view *datastore.IndexView, c candidates) int {
	if c.all {
		return kindCount(view, datastore.KeyPresent)
	}

	n := c.set.Count()
	if c.exact != nil {
		n -= c.exact.Count()
	}

	return n
}

func evaluateLeaf(view *datastore.IndexView, ops map[string]string, current candidates) (candidates, bool) {
	if len(ops) == 0 || pendingCount(view, current) < evalMinPending {
		return current, false
	}

	if c, ok := dateRangeOnIndex(view, ops); ok {
		return c, true
	}

	scalars := kindCount(view, datastore.KeyRaw) + kindCount(view, datastore.KeyNumber) + kindCount(view, datastore.KeyBool)
	if pendingCount(view, current)*4 < scalars {
		return current, false
	}

	exact := &datastore.Bitset{}
	scanKind(view, datastore.KeyRaw, func(key string, seq uint32) {
		if !exact.Has(seq) && matchStringOrDate(unescapePayload(key), ops) {
			exact.Set(seq)
		}
	})
	scanKind(view, datastore.KeyNumber, func(key string, seq uint32) {
		if !exact.Has(seq) && matchNumber(numberOfKey(key), ops) {
			exact.Set(seq)
		}
	})
	scanKind(view, datastore.KeyBool, func(key string, seq uint32) {
		if !exact.Has(seq) && matchBoolean(key[1] == 1, ops) {
			exact.Set(seq)
		}
	})

	set := exact.Clone()
	if arraysMayMatch(ops) {
		lo, hi := kindBounds(datastore.KeyElement)
		if emptyArraysMayMatch(ops) {
			lo, hi = kindBounds(datastore.KeyPresent)
		}
		collect(view, lo, hi, set, nil)
	}

	return candidates{set: set, exact: exact}, true
}

func scanKind(view *datastore.IndexView, kind byte, fn func(key string, seq uint32)) {
	lo, hi := kindBounds(kind)
	view.ScanRange(lo, hi, func(key string, seq uint32) bool {
		fn(key, seq)
		return true
	})
}

func numberOfKey(key string) float64 {
	bits := binary.BigEndian.Uint64([]byte(key[1:9]))
	if bits&(1<<63) != 0 {
		bits &^= 1 << 63
	} else {
		bits = ^bits
	}

	return math.Float64frombits(bits)
}

func timeOfKey(key string) time.Time {
	b := []byte(key[1:13])
	sec := int64(binary.BigEndian.Uint64(b[:8]) ^ (1 << 63))

	return time.Unix(sec, int64(binary.BigEndian.Uint32(b[8:12])))
}

func arraysMayMatch(ops map[string]string) bool {
	for op := range ops {
		switch op {
		case "contains", "not_contains", "all", "any", "eq", "none":
		default:
			return false
		}
	}

	return true
}

func emptyArraysMayMatch(ops map[string]string) bool {
	for op := range ops {
		if op != "not_contains" && op != "none" {
			return false
		}
	}

	return true
}

func dateRangeOnIndex(view *datastore.IndexView, ops map[string]string) (candidates, bool) {
	var lo, hi *time.Time
	for op, cmp := range ops {
		t, ok, dateOnly := docpath.ParseDate(cmp)
		if !ok || !dateOnly {
			return candidates{}, false
		}

		switch op {
		case "gte", "gt":
			if op == "gt" {
				t = t.Add(24 * time.Hour)
			}
			if lo == nil || t.After(*lo) {
				lo = &t
			}
		case "lt", "lte":
			if op == "lte" {
				t = t.Add(24 * time.Hour)
			}
			if hi == nil || t.Before(*hi) {
				hi = &t
			}
		default:
			return candidates{}, false
		}
	}

	if kindCount(view, datastore.KeyDate) != kindCount(view, datastore.KeyRaw) {
		return candidates{}, false
	}

	l, h := kindBounds(datastore.KeyDate)
	earliest := true
	view.ScanRange(l, h, func(key string, _ uint32) bool {
		earliest = !timeOfKey(key).Before(zeroTime)
		return false
	})
	if !earliest {
		return candidates{}, false
	}

	if lo != nil {
		l = datastore.AppendTime([]byte{datastore.KeyDate}, *lo)
	}
	if hi != nil {
		h = datastore.AppendTime([]byte{datastore.KeyDate}, *hi)
	}

	exact := &datastore.Bitset{}
	if lo == nil || hi == nil || lo.Before(*hi) {
		collect(view, l, h, exact, nil)
	}

	return candidates{set: exact.Clone(), exact: exact}, true
}
