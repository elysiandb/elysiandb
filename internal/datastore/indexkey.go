package datastore

import (
	"encoding/binary"
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/docpath"
)

type IndexKind byte

const (
	IndexFilter IndexKind = 'f'
	IndexSort   IndexKind = 's'
	IndexRefs   IndexKind = 'r'
	IndexSearch IndexKind = 'g'
)

type IndexSpec struct {
	Kind  IndexKind
	Field string
}

func (s IndexSpec) Name() string {
	return string(s.Kind) + ":" + s.Field
}

func ParseIndexName(name string) (IndexSpec, bool) {
	if len(name) < 2 || name[1] != ':' {
		return IndexSpec{}, false
	}

	switch IndexKind(name[0]) {
	case IndexFilter, IndexSort, IndexRefs, IndexSearch:
		return IndexSpec{Kind: IndexKind(name[0]), Field: name[2:]}, true
	}

	return IndexSpec{}, false
}

const (
	KeyNumber  byte = 'n'
	KeyDate    byte = 'd'
	KeyLower   byte = 'l'
	KeyRaw     byte = 'r'
	KeyBool    byte = 'b'
	KeyElement byte = 'a'
	KeyPresent byte = 'p'
)

const (
	SortRankNumber byte = iota
	SortRankTime
	SortRankString
	SortRankBool
	SortRankOther
	SortRankMissing
)

func AppendEscaped(dst []byte, s string) []byte {
	for i := 0; i < len(s); i++ {
		if s[i] == 0 {
			dst = append(dst, 0, 0xff)
			continue
		}

		dst = append(dst, s[i])
	}

	return dst
}

func AppendTerminated(dst []byte, s string) []byte {
	return append(AppendEscaped(dst, s), 0, 1)
}

func AppendNumber(dst []byte, f float64) []byte {
	if f == 0 {
		f = 0
	}

	bits := math.Float64bits(f)
	if bits&(1<<63) != 0 {
		bits = ^bits
	} else {
		bits |= 1 << 63
	}

	return binary.BigEndian.AppendUint64(dst, bits)
}

func AppendTime(dst []byte, t time.Time) []byte {
	dst = binary.BigEndian.AppendUint64(dst, uint64(t.Unix())^(1<<63))

	return binary.BigEndian.AppendUint32(dst, uint32(t.Nanosecond()))
}

func seqKey(seq uint32) string {
	var b [4]byte
	binary.BigEndian.PutUint32(b[:], seq)

	return string(b[:])
}

func keySeq(key string) uint32 {
	return binary.BigEndian.Uint32([]byte(key[len(key)-4:]))
}

func KeyPayload(key string) string {
	return key[:len(key)-4]
}

func entryKey(payload []byte, seq uint32) string {
	return string(binary.BigEndian.AppendUint32(payload, seq))
}

func ElementString(v any) string {
	switch s := v.(type) {
	case string:
		return s
	case float64:
		return strconv.FormatFloat(s, 'f', -1, 64)
	case int:
		return strconv.Itoa(s)
	default:
		return fmt.Sprintf("%v", s)
	}
}

func FilterKeys(doc map[string]any, field string) [][]byte {
	if doc == nil {
		return nil
	}

	values := docpath.ResolveValues(doc, field)
	if len(values) == 0 {
		return nil
	}

	keys := [][]byte{{KeyPresent}}
	for _, value := range values {
		keys = appendValueKeys(keys, value)
	}

	return keys
}

func appendValueKeys(keys [][]byte, value any) [][]byte {
	switch v := value.(type) {
	case string:
		keys = append(keys, AppendTerminated([]byte{KeyLower}, strings.ToLower(v)))
		keys = append(keys, AppendTerminated([]byte{KeyRaw}, v))
		if t, ok, _ := docpath.ParseDate(v); ok {
			keys = append(keys, AppendTime([]byte{KeyDate}, t))
		}
	case float64:
		keys = append(keys, AppendNumber([]byte{KeyNumber}, v))
	case int:
		keys = append(keys, AppendNumber([]byte{KeyNumber}, float64(v)))
	case int32:
		keys = append(keys, AppendNumber([]byte{KeyNumber}, float64(v)))
	case int64:
		keys = append(keys, AppendNumber([]byte{KeyNumber}, float64(v)))
	case bool:
		b := byte(0)
		if v {
			b = 1
		}
		keys = append(keys, []byte{KeyBool, b})
	case []any:
		for _, item := range v {
			keys = append(keys, AppendTerminated([]byte{KeyElement}, ElementString(item)))
		}
	}

	return keys
}

func SortKey(doc map[string]any, field string) []byte {
	if doc == nil {
		return []byte{SortRankMissing}
	}

	return SortValueKey(docpath.SortValue(doc, field))
}

func SortValueKey(v any) []byte {
	switch t := v.(type) {
	case nil:
		return []byte{SortRankMissing}
	case float64:
		return AppendNumber([]byte{SortRankNumber}, t)
	case float32:
		return AppendNumber([]byte{SortRankNumber}, float64(t))
	case int:
		return AppendNumber([]byte{SortRankNumber}, float64(t))
	case int32:
		return AppendNumber([]byte{SortRankNumber}, float64(t))
	case int64:
		return AppendNumber([]byte{SortRankNumber}, float64(t))
	case time.Time:
		return AppendTime([]byte{SortRankTime}, t)
	case string:
		return AppendTerminated([]byte{SortRankString}, t)
	case bool:
		if t {
			return []byte{SortRankBool, 1}
		}
		return []byte{SortRankBool, 0}
	default:
		return []byte{SortRankOther}
	}
}

type RefPath struct {
	Path   string
	Target string
}

func IsReference(m map[string]any) (string, string, bool) {
	ent, ok := m["@entity"].(string)
	if !ok || ent == "" {
		return "", "", false
	}

	id, _ := m["id"].(string)

	return ent, id, true
}

func DocumentRefs(doc map[string]any) []RefPath {
	var out []RefPath
	var walk func(v any, path string)
	walk = func(v any, path string) {
		switch t := v.(type) {
		case map[string]any:
			if path != "" {
				if target, _, ok := IsReference(t); ok {
					out = append(out, RefPath{Path: path, Target: target})
					return
				}
			}

			for k, child := range t {
				p := k
				if path != "" {
					p = path + "." + k
				}
				walk(child, p)
			}
		case []any:
			for _, item := range t {
				walk(item, path)
			}
		}
	}

	walk(doc, "")

	return out
}

func RefKeys(doc map[string]any) [][]byte {
	var keys [][]byte
	var walk func(v any, path string)
	walk = func(v any, path string) {
		switch t := v.(type) {
		case map[string]any:
			if path != "" {
				if target, id, ok := IsReference(t); ok {
					keys = append(keys, RefKey(path, target, id))
					return
				}
			}

			for k, child := range t {
				p := k
				if path != "" {
					p = path + "." + k
				}
				walk(child, p)
			}
		case []any:
			for _, item := range t {
				walk(item, path)
			}
		}
	}

	walk(doc, "")

	return keys
}

func RefKey(path, target, id string) []byte {
	b := AppendTerminated(nil, path)
	b = AppendTerminated(b, target)

	return AppendTerminated(b, id)
}

func RefPrefix(path, target string) []byte {
	return AppendTerminated(AppendTerminated(nil, path), target)
}

func extractKeys(spec IndexSpec, doc map[string]any) [][]byte {
	if doc == nil {
		return nil
	}

	switch spec.Kind {
	case IndexFilter:
		return FilterKeys(doc, spec.Field)
	case IndexSort:
		return [][]byte{SortKey(doc, spec.Field)}
	case IndexRefs:
		return RefKeys(doc)
	}

	return nil
}

func uniqueKeys(keys [][]byte) []string {
	if len(keys) == 0 {
		return nil
	}

	out := make([]string, 0, len(keys))
	for _, k := range keys {
		out = append(out, string(k))
	}

	sort.Strings(out)
	n := 0
	for i, k := range out {
		if i == 0 || k != out[n-1] {
			out[n] = k
			n++
		}
	}

	return out[:n]
}
