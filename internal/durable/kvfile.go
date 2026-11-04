package durable

import (
	"encoding/binary"
	"errors"
	"hash/crc32"
	"io"
)

const kvFileMagic = "ELYKVCK1"

var ErrCorruptFile = errors.New("durable: file is corrupt")

type KVRecord struct {
	Key       string
	LSN       uint64
	ExpiresAt int64
	Segment   uint32
	Offset    uint32
	Length    uint32
}

func WriteKVFile(f FS, path string, records []KVRecord) error {
	return WriteFileAtomic(f, path, func(w io.Writer) error {
		crc := crc32.New(castagnoli)
		out := io.MultiWriter(w, crc)
		buf := make([]byte, 0, 64)
		put := func(b []byte) error {
			_, err := out.Write(b)
			return err
		}

		if err := put([]byte(kvFileMagic)); err != nil {
			return err
		}

		if err := put(binary.AppendUvarint(buf[:0], uint64(len(records)))); err != nil {
			return err
		}

		for _, r := range records {
			buf = binary.AppendUvarint(buf[:0], uint64(len(r.Key)))
			buf = append(buf, r.Key...)
			buf = binary.AppendUvarint(buf, r.LSN)
			buf = binary.AppendVarint(buf, r.ExpiresAt)
			buf = binary.AppendUvarint(buf, uint64(r.Segment))
			buf = binary.AppendUvarint(buf, uint64(r.Offset))
			buf = binary.AppendUvarint(buf, uint64(r.Length))
			if err := put(buf); err != nil {
				return err
			}
		}

		var sum [4]byte
		binary.LittleEndian.PutUint32(sum[:], crc.Sum32())
		_, err := w.Write(sum[:])

		return err
	})
}

func ReadKVFile(f FS, path string) ([]KVRecord, error) {
	data, err := ReadFile(f, path)
	if err != nil {
		return nil, err
	}

	if len(data) < len(kvFileMagic)+4 || string(data[:len(kvFileMagic)]) != kvFileMagic {
		return nil, ErrCorruptFile
	}

	body := data[:len(data)-4]
	if crc32.Checksum(body, castagnoli) != binary.LittleEndian.Uint32(data[len(data)-4:]) {
		return nil, ErrCorruptFile
	}

	pos := len(kvFileMagic)
	bad := false
	uv := func() uint64 {
		v, n := binary.Uvarint(body[pos:])
		if n <= 0 {
			bad = true
			return 0
		}
		pos += n
		return v
	}
	sv := func() int64 {
		v, n := binary.Varint(body[pos:])
		if n <= 0 {
			bad = true
			return 0
		}
		pos += n
		return v
	}

	var out []KVRecord
	for n := uv(); n > 0 && !bad; n-- {
		l := uv()
		if bad || uint64(len(body)-pos) < l {
			return nil, ErrCorruptFile
		}

		r := KVRecord{Key: string(body[pos : pos+int(l)])}
		pos += int(l)
		r.LSN = uv()
		r.ExpiresAt = sv()
		r.Segment = uint32(uv())
		r.Offset = uint32(uv())
		r.Length = uint32(uv())
		out = append(out, r)
	}

	if bad || pos != len(body) {
		return nil, ErrCorruptFile
	}

	return out, nil
}
