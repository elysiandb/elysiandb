package journal

import (
	"encoding/binary"
	"errors"
	"fmt"
	"hash/crc32"
)

type Kind uint8

const (
	KindDocPut Kind = iota + 1
	KindDocDelete
	KindDocMove
	KindEntityReset
	KindEntityCreate
	KindEntityDrop
	KindKVPut
	KindKVDelete
	KindKVExpire
	KindKVMove
	KindKVReset
	KindClearAll
)

func (k Kind) String() string {
	switch k {
	case KindDocPut:
		return "doc_put"
	case KindDocDelete:
		return "doc_delete"
	case KindDocMove:
		return "doc_move"
	case KindEntityReset:
		return "entity_reset"
	case KindEntityCreate:
		return "entity_create"
	case KindEntityDrop:
		return "entity_drop"
	case KindKVPut:
		return "kv_put"
	case KindKVDelete:
		return "kv_delete"
	case KindKVExpire:
		return "kv_expire"
	case KindKVMove:
		return "kv_move"
	case KindKVReset:
		return "kv_reset"
	case KindClearAll:
		return "clear_all"
	default:
		return fmt.Sprintf("kind_%d", uint8(k))
	}
}

type Location struct {
	Segment uint32
	Offset  uint32
	Length  uint32
}

type Position struct {
	Segment uint32
	Offset  uint32
}

func (p Position) Less(o Position) bool {
	if p.Segment != o.Segment {
		return p.Segment < o.Segment
	}

	return p.Offset < o.Offset
}

type Record struct {
	Kind      Kind
	LSN       uint64
	Gen       uint64
	ExpiresAt int64
	Entity    string
	Key       string
	Value     []byte
	Loc       Location
}

const (
	segmentMagic      = "ELYSLOG1"
	segmentHeaderSize = 16
	segmentVersion    = 1
	batchMagic        = uint32(0x31424c45)
	batchHeaderSize   = 16
	maxRecordsInBatch = 1 << 24
)

var (
	castagnoli    = crc32.MakeTable(crc32.Castagnoli)
	ErrCorrupt    = errors.New("journal: corrupt batch")
	ErrTornBatch  = errors.New("journal: torn batch")
	ErrBadSegment = errors.New("journal: not a segment file")
)

func batchSize(recs []Record) int {
	size := batchHeaderSize
	for i := range recs {
		size += recordSize(&recs[i])
	}

	return size
}

func appendBatch(dst []byte, recs []Record, firstLSN uint64, base uint32, segment uint32) []byte {
	start := len(dst)
	dst = append(dst, make([]byte, batchHeaderSize)...)
	for i := range recs {
		r := &recs[i]
		r.LSN = firstLSN + uint64(i)
		dst = append(dst, byte(r.Kind))
		dst = binary.LittleEndian.AppendUint64(dst, r.LSN)
		dst = binary.AppendUvarint(dst, r.Gen)
		dst = binary.AppendVarint(dst, r.ExpiresAt)
		dst = binary.AppendUvarint(dst, uint64(len(r.Entity)))
		dst = append(dst, r.Entity...)
		dst = binary.AppendUvarint(dst, uint64(len(r.Key)))
		dst = append(dst, r.Key...)
		dst = binary.AppendUvarint(dst, uint64(len(r.Value)))
		r.Loc = Location{Segment: segment, Offset: base + uint32(len(dst)-start), Length: uint32(len(r.Value))}
		dst = append(dst, r.Value...)
	}

	payload := dst[start+batchHeaderSize:]
	binary.LittleEndian.PutUint32(dst[start:], batchMagic)
	binary.LittleEndian.PutUint32(dst[start+4:], uint32(len(payload)))
	binary.LittleEndian.PutUint32(dst[start+8:], crc32.Checksum(payload, castagnoli))
	binary.LittleEndian.PutUint32(dst[start+12:], uint32(len(recs)))

	return dst
}

type encodedBatch struct {
	data       []byte
	lsnOffsets []int
	valOffsets []int
}

func encodeBatch(recs []Record) encodedBatch {
	size := batchHeaderSize
	for i := range recs {
		size += recordSize(&recs[i])
	}

	b := encodedBatch{data: make([]byte, batchHeaderSize, size), lsnOffsets: make([]int, len(recs)), valOffsets: make([]int, len(recs))}
	for i := range recs {
		r := &recs[i]
		b.data = append(b.data, byte(r.Kind))
		b.lsnOffsets[i] = len(b.data)
		b.data = binary.LittleEndian.AppendUint64(b.data, 0)
		b.data = binary.AppendUvarint(b.data, r.Gen)
		b.data = binary.AppendVarint(b.data, r.ExpiresAt)
		b.data = binary.AppendUvarint(b.data, uint64(len(r.Entity)))
		b.data = append(b.data, r.Entity...)
		b.data = binary.AppendUvarint(b.data, uint64(len(r.Key)))
		b.data = append(b.data, r.Key...)
		b.data = binary.AppendUvarint(b.data, uint64(len(r.Value)))
		b.valOffsets[i] = len(b.data)
		b.data = append(b.data, r.Value...)
	}

	return b
}

func recordSize(r *Record) int {
	return 1 + 8 + 3*binary.MaxVarintLen64 + len(r.Entity) + len(r.Key) + len(r.Value) + binary.MaxVarintLen64
}

func sealBatch(b encodedBatch, recs []Record, firstLSN uint64) {
	for i, off := range b.lsnOffsets {
		lsn := firstLSN + uint64(i)
		recs[i].LSN = lsn
		binary.LittleEndian.PutUint64(b.data[off:], lsn)
	}

	payload := b.data[batchHeaderSize:]
	binary.LittleEndian.PutUint32(b.data[0:], batchMagic)
	binary.LittleEndian.PutUint32(b.data[4:], uint32(len(payload)))
	binary.LittleEndian.PutUint32(b.data[8:], crc32.Checksum(payload, castagnoli))
	binary.LittleEndian.PutUint32(b.data[12:], uint32(len(recs)))
}

func segmentHeader(id uint32) []byte {
	h := make([]byte, segmentHeaderSize)
	copy(h, segmentMagic)
	binary.LittleEndian.PutUint32(h[8:], id)
	binary.LittleEndian.PutUint32(h[12:], segmentVersion)

	return h
}

func checkSegmentHeader(h []byte, id uint32) error {
	if len(h) < segmentHeaderSize || string(h[:8]) != segmentMagic {
		return ErrBadSegment
	}

	if got := binary.LittleEndian.Uint32(h[8:]); got != id {
		return fmt.Errorf("%w: header names segment %d, file is %d", ErrBadSegment, got, id)
	}

	return nil
}

func decodeBatch(data []byte, segment uint32, base uint32) ([]Record, int, error) {
	if len(data) < batchHeaderSize {
		return nil, 0, ErrTornBatch
	}

	if binary.LittleEndian.Uint32(data[0:]) != batchMagic {
		return nil, 0, ErrCorrupt
	}

	payloadLen := int(binary.LittleEndian.Uint32(data[4:]))
	if batchHeaderSize+payloadLen > len(data) {
		return nil, 0, ErrTornBatch
	}

	payload := data[batchHeaderSize : batchHeaderSize+payloadLen]
	if crc32.Checksum(payload, castagnoli) != binary.LittleEndian.Uint32(data[8:]) {
		return nil, 0, ErrCorrupt
	}

	count := int(binary.LittleEndian.Uint32(data[12:]))
	if count > maxRecordsInBatch {
		return nil, 0, ErrCorrupt
	}

	recs := make([]Record, 0, count)
	pos := 0
	for i := 0; i < count; i++ {
		r, n, err := decodeRecord(payload[pos:])
		if err != nil {
			return nil, 0, err
		}

		valueStart := base + uint32(batchHeaderSize+pos+n-len(r.Value))
		r.Loc = Location{Segment: segment, Offset: valueStart, Length: uint32(len(r.Value))}
		recs = append(recs, r)
		pos += n
	}

	if pos != len(payload) {
		return nil, 0, ErrCorrupt
	}

	return recs, batchHeaderSize + payloadLen, nil
}

func decodeRecord(b []byte) (Record, int, error) {
	var r Record
	if len(b) < 9 {
		return r, 0, ErrCorrupt
	}

	r.Kind = Kind(b[0])
	r.LSN = binary.LittleEndian.Uint64(b[1:])
	pos := 9

	gen, n := binary.Uvarint(b[pos:])
	if n <= 0 {
		return r, 0, ErrCorrupt
	}
	r.Gen = gen
	pos += n

	exp, n := binary.Varint(b[pos:])
	if n <= 0 {
		return r, 0, ErrCorrupt
	}
	r.ExpiresAt = exp
	pos += n

	entity, n, ok := readBytes(b[pos:])
	if !ok {
		return r, 0, ErrCorrupt
	}
	r.Entity = string(entity)
	pos += n

	key, n, ok := readBytes(b[pos:])
	if !ok {
		return r, 0, ErrCorrupt
	}
	r.Key = string(key)
	pos += n

	value, n, ok := readBytes(b[pos:])
	if !ok {
		return r, 0, ErrCorrupt
	}
	r.Value = value
	pos += n

	return r, pos, nil
}

func readBytes(b []byte) ([]byte, int, bool) {
	l, n := binary.Uvarint(b)
	if n <= 0 || uint64(len(b)-n) < l {
		return nil, 0, false
	}

	return b[n : n+int(l)], n + int(l), true
}
