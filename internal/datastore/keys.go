package datastore

import (
	"encoding/binary"
	"strings"
)

const (
	familyMeta   byte = 0x01
	familyEntity byte = 0x02
	familyKV     byte = 0x03
	familyTTL    byte = 0x04
)

const (
	metaFormat byte = 'f'
	metaTypes  byte = 't'
	metaEntity byte = 'e'
)

const (
	partDocs    byte = 'd'
	partIDs     byte = 'i'
	partIndexes byte = 'x'
)

func metaKey(kind byte) []byte {
	return []byte{familyMeta, kind}
}

func entityMetaKey(name string) []byte {
	return append([]byte{familyMeta, metaEntity}, name...)
}

func entityPrefix(id uint32) []byte {
	b := make([]byte, 5, 16)
	b[0] = familyEntity
	binary.BigEndian.PutUint32(b[1:], id)

	return b
}

func entityRange(id uint32) ([]byte, []byte) {
	return entityPrefix(id), prefixEnd(entityPrefix(id))
}

func partPrefix(id uint32, part byte) []byte {
	return append(entityPrefix(id), part)
}

func docKey(id uint32, seq uint32) []byte {
	b := make([]byte, 10)
	b[0] = familyEntity
	binary.BigEndian.PutUint32(b[1:], id)
	b[5] = partDocs
	binary.BigEndian.PutUint32(b[6:], seq)

	return b
}

func idKey(id uint32, docID string) []byte {
	b := make([]byte, 6, 6+len(docID))
	b[0] = familyEntity
	binary.BigEndian.PutUint32(b[1:], id)
	b[5] = partIDs

	return append(b, docID...)
}

func indexPrefix(id uint32, no uint16) []byte {
	b := make([]byte, 8, 64)
	b[0] = familyEntity
	binary.BigEndian.PutUint32(b[1:], id)
	b[5] = partIndexes
	binary.BigEndian.PutUint16(b[6:], no)

	return b
}

func indexKey(id uint32, no uint16, entry []byte) []byte {
	return append(indexPrefix(id, no), entry...)
}

func indexRange(id uint32, no uint16) ([]byte, []byte) {
	p := indexPrefix(id, no)

	return p, prefixEnd(p)
}

func prefixEnd(p []byte) []byte {
	end := append([]byte(nil), p...)
	for i := len(end) - 1; i >= 0; i-- {
		if end[i] < 0xff {
			end[i]++
			return end[:i+1]
		}
	}

	return nil
}

func kvKey(key string) []byte {
	lower := strings.ToLower(key)
	b := make([]byte, 0, 2+len(lower)+len(key))
	b = append(b, familyKV)
	b = append(b, lower...)
	b = append(b, 0)

	return append(b, key...)
}

func kvSeekKey(lowerPrefix string) []byte {
	return append([]byte{familyKV}, lowerPrefix...)
}

func ttlKey(expiresAt int64, key string) []byte {
	b := make([]byte, 9, 9+len(key))
	b[0] = familyTTL
	binary.BigEndian.PutUint64(b[1:], uint64(expiresAt))

	return append(b, key...)
}

func encodeKVValue(dst []byte, key string, value []byte, expiresAt int64) []byte {
	dst = binary.AppendVarint(dst, expiresAt)
	dst = binary.AppendUvarint(dst, uint64(len(key)))

	return append(dst, value...)
}

func decodeKVValue(pebbleKey, b []byte) (string, []byte, int64, bool) {
	exp, n := binary.Varint(b)
	if n <= 0 {
		return "", nil, 0, false
	}

	keyLen, m := binary.Uvarint(b[n:])
	if m <= 0 || keyLen > uint64(len(pebbleKey)) {
		return "", nil, 0, false
	}

	key := string(pebbleKey[len(pebbleKey)-int(keyLen):])

	return key, b[n+m:], exp, true
}

func encodeDocValue(dst []byte, lsn uint64, id string, value []byte) []byte {
	dst = binary.AppendUvarint(dst, lsn)
	dst = binary.AppendUvarint(dst, uint64(len(id)))
	dst = append(dst, id...)

	return append(dst, value...)
}

func decodeDocValue(b []byte) (uint64, []byte, []byte, bool) {
	lsn, n := binary.Uvarint(b)
	if n <= 0 {
		return 0, nil, nil, false
	}

	idLen, m := binary.Uvarint(b[n:])
	if m <= 0 || uint64(len(b)-n-m) < idLen {
		return 0, nil, nil, false
	}

	start := n + m

	return lsn, b[start : start+int(idLen)], b[start+int(idLen):], true
}
