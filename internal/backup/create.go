package backup

import (
	"archive/tar"
	"bufio"
	"compress/gzip"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sync"
	"time"
	"unicode/utf8"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/durable"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

const (
	beatEvery    = time.Second
	maxReadPause = 100 * time.Millisecond
)

type Options struct {
	Beat  func()
	Flush func() error
	Pace  bool
}

type entry struct {
	name     string
	write    func(w io.Writer) error
	size     int64
	measured bool
}

type Source struct {
	now      time.Time
	manifest *Manifest
	entries  []*entry
	release  func()
	once     sync.Once
}

func Prepare(now time.Time) (*Source, error) {
	manifest := &Manifest{
		Format:           Format,
		FormatVersion:    FormatVersion,
		ElysianDBVersion: globals.VERSION,
		CreatedAt:        now.UTC().Format(time.RFC3339),
		Engine:           globals.GetEngine(),
		Entities:         []EntityManifest{},
		Schemas:          []string{},
		Files:            map[string]string{},
	}

	key, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		return nil, err
	}

	s := &Source{now: now, manifest: manifest, release: func() {}}
	s.entries = append(s.entries, &entry{name: KeyPath, write: func(w io.Writer) error {
		_, err := io.WriteString(w, key)
		return err
	}})

	e := storage.Engine()
	if !engine.IsEngineInternal() || e == nil {
		if err := s.spool(now); err != nil {
			s.Close()
			return nil, err
		}

		return s, nil
	}

	point := e.PointInTime()
	s.release = point.Release

	if err := s.addSchemas(point); err != nil {
		s.Close()
		return nil, err
	}

	s.addEntities(point)
	s.addKeys(point)

	return s, nil
}

func (s *Source) Close() {
	s.once.Do(s.release)
}

func Create(w io.Writer, now time.Time) (*Manifest, error) {
	src, err := Prepare(now)
	if err != nil {
		return nil, err
	}
	defer src.Close()

	if err := src.WriteArchive(w, Options{}); err != nil {
		return nil, err
	}

	return src.manifest, nil
}

func WriteFile(folder string, now time.Time) (string, *Manifest, error) {
	return writeFile(folder, now, Options{})
}

func writeFile(folder string, now time.Time, opts Options) (string, *Manifest, error) {
	path := filepath.Join(folder, FileName(now))
	if _, err := os.Stat(path); err == nil {
		return "", nil, fmt.Errorf("%s already exists", path)
	}

	tmp, err := os.CreateTemp(folder, "."+FileName(now)+".*")
	if err != nil {
		return "", nil, err
	}

	defer os.Remove(tmp.Name())

	src, err := Prepare(now)
	if err != nil {
		tmp.Close()
		return "", nil, err
	}
	defer src.Close()

	err = src.WriteArchive(tmp, opts)
	if err == nil {
		err = tmp.Sync()
	}

	if closeErr := tmp.Close(); err == nil {
		err = closeErr
	}

	if err != nil {
		return "", nil, err
	}

	if err := os.Rename(tmp.Name(), path); err != nil {
		return "", nil, err
	}

	if err := durable.OS.SyncDir(folder); err != nil {
		return "", nil, err
	}

	return path, src.manifest, nil
}

func (s *Source) addSchemas(point *datastore.PointInTime) error {
	snap, ok := point.Snapshots[schema.SchemaEntity]
	if !ok {
		return nil
	}

	return api_storage.IterateSnapshot(snap, func(data map[string]any) error {
		entity, _ := data["id"].(string)
		if entity == "" {
			return nil
		}

		s.manifest.Schemas = append(s.manifest.Schemas, entity)
		s.entries = append(s.entries, &entry{name: SchemaPath(entity), write: func(w io.Writer) error {
			return newEncoder(w).Encode(data)
		}})

		return nil
	})
}

func (s *Source) addEntities(point *datastore.PointInTime) {
	seen := map[string]bool{}
	for _, name := range point.Types {
		if name == schema.SchemaEntity || seen[name] {
			continue
		}

		seen[name] = true
		s.manifest.Entities = append(s.manifest.Entities, EntityManifest{Name: name})
		slot := len(s.manifest.Entities) - 1
		snap := point.Snapshots[name]

		s.entries = append(s.entries, &entry{name: EntityPath(name), write: func(w io.Writer) error {
			if snap == nil {
				return nil
			}

			count, err := writeDocuments(w, snap, pauseFor(w))
			s.manifest.Entities[slot].Documents = count

			return err
		}})
	}
}

func (s *Source) addKeys(point *datastore.PointInTime) {
	now := point.At.Unix()
	s.entries = append(s.entries, &entry{name: KVPath, write: func(w io.Writer) error {
		encoder := newEncoder(w)
		count := 0
		for _, item := range point.KV {
			if globals.IsApiKey(item.Key) || (item.ExpiresAt > 0 && now >= item.ExpiresAt) {
				continue
			}

			kv := newKVEntry(item.Key, item.Value)
			if item.ExpiresAt > 0 {
				kv.ExpiresAt = item.ExpiresAt
			}

			if err := encoder.Encode(kv); err != nil {
				return err
			}

			count++
		}
		s.manifest.KVKeys = count

		return nil
	}})
}

type paced interface {
	pause(time.Duration)
}

func pauseFor(w io.Writer) func(time.Duration) {
	if p, ok := w.(paced); ok {
		return p.pause
	}

	return nil
}

func writeDocuments(w io.Writer, snap *datastore.Snapshot, pace func(time.Duration)) (int, error) {
	encoder := newEncoder(w)
	count := 0
	newline := []byte{'\n'}

	var failed error
	snap.ScanValuesPaced(pace, func(d datastore.Doc, raw []byte, err error) bool {
		if err != nil {
			failed = err
			return false
		}

		if !json.Valid(raw) {
			return true
		}

		if id, ok := simpleDocumentID(raw); ok && id != "" {
			count++
			if _, failed = w.Write(raw); failed != nil {
				return false
			}
			_, failed = w.Write(newline)
			return failed == nil
		}

		data, decodeErr := datastore.DecodeMap(raw)
		if decodeErr != nil || data == nil {
			return true
		}

		if id, _ := data["id"].(string); id == "" {
			return true
		}

		count++
		failed = encoder.Encode(data)

		return failed == nil
	})

	return count, failed
}

func simpleDocumentID(raw []byte) (string, bool) {
	i := skipSpace(raw, 0)
	if i >= len(raw) || raw[i] != '{' {
		return "", false
	}
	i++

	id := ""
	found := false
	for {
		i = skipSpace(raw, i)
		if i >= len(raw) {
			return "", false
		}

		if raw[i] == '}' {
			return id, found
		}

		if raw[i] == ',' {
			i++
			continue
		}

		keyStart, keyEnd, next, plain := scanString(raw, i)
		if next < 0 || !plain {
			return "", false
		}

		i = skipSpace(raw, next)
		if i >= len(raw) || raw[i] != ':' {
			return "", false
		}
		i = skipSpace(raw, i+1)

		isID := string(raw[keyStart:keyEnd]) == "id"
		if isID {
			if i >= len(raw) || raw[i] != '"' {
				return "", false
			}

			valStart, valEnd, after, valPlain := scanString(raw, i)
			if after < 0 || !valPlain {
				return "", false
			}

			id = string(raw[valStart:valEnd])
			found = true
			i = after
			continue
		}

		if i = skipValue(raw, i); i < 0 {
			return "", false
		}
	}
}

func skipSpace(raw []byte, i int) int {
	for i < len(raw) && (raw[i] == ' ' || raw[i] == '\n' || raw[i] == '\t' || raw[i] == '\r') {
		i++
	}

	return i
}

func scanString(raw []byte, i int) (int, int, int, bool) {
	if i >= len(raw) || raw[i] != '"' {
		return 0, 0, -1, false
	}

	start := i + 1
	plain := true
	for j := start; j < len(raw); j++ {
		switch raw[j] {
		case '\\':
			plain = false
			j++
		case '"':
			return start, j, j + 1, plain
		}
	}

	return 0, 0, -1, false
}

func skipValue(raw []byte, i int) int {
	depth := 0
	for i < len(raw) {
		switch raw[i] {
		case '"':
			_, _, next, _ := scanString(raw, i)
			if next < 0 {
				return -1
			}
			i = next
			if depth == 0 {
				return i
			}
			continue
		case '{', '[':
			depth++
		case '}', ']':
			if depth == 0 {
				return i
			}
			depth--
			if depth == 0 {
				return i + 1
			}
		case ',':
			if depth == 0 {
				return i
			}
		}
		i++
	}

	return -1
}

func newKVEntry(key string, value []byte) KVEntry {
	if utf8.Valid(value) {
		return KVEntry{Key: key, Value: string(value)}
	}

	return KVEntry{Key: key, Value: base64.StdEncoding.EncodeToString(value), Encoding: KVEncodingBase64}
}

type sumWriter struct {
	hash  io.Writer
	out   io.Writer
	size  int64
	beats func()
	pace  func(time.Duration)
}

func (w *sumWriter) Write(p []byte) (int, error) {
	w.size += int64(len(p))
	_, _ = w.hash.Write(p)
	if w.beats != nil {
		w.beats()
	}

	if w.out == nil {
		return len(p), nil
	}

	return w.out.Write(p)
}

func (w *sumWriter) pause(took time.Duration) {
	if w.pace != nil {
		w.pace(took)
	}
}

type bufferedSum struct {
	*bufio.Writer
	inner *sumWriter
}

func (b bufferedSum) pause(took time.Duration) {
	b.inner.pause(took)
}

func (e *entry) run(out io.Writer, beats func(), pace func(time.Duration)) (string, int64, error) {
	hash := sha256.New()
	sum := &sumWriter{hash: hash, out: out, beats: beats, pace: pace}
	buffered := bufferedSum{Writer: bufio.NewWriterSize(sum, 64<<10), inner: sum}

	if err := e.write(buffered); err != nil {
		return "", 0, err
	}

	if err := buffered.Flush(); err != nil {
		return "", 0, err
	}

	return hex.EncodeToString(hash.Sum(nil)), sum.size, nil
}

type countingWriter struct {
	w io.Writer
	n int64
}

func (c *countingWriter) Write(p []byte) (int, error) {
	n, err := c.w.Write(p)
	c.n += int64(n)

	return n, err
}

func (s *Source) WriteArchive(w io.Writer, opts Options) (err error) {
	done := begin()
	counted := &countingWriter{w: w}
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("the backup stopped: %v", r)
		}
		done(s.manifest, counted.n, err)
	}()

	gz, err := gzip.NewWriterLevel(counted, gzip.BestSpeed)
	if err != nil {
		return err
	}

	last := time.Now()
	beat := func(flushGzip bool) {
		if time.Since(last) < beatEvery {
			return
		}
		last = time.Now()

		if flushGzip {
			_ = gz.Flush()
			if opts.Flush != nil {
				_ = opts.Flush()
			}
		}

		if opts.Beat != nil {
			opts.Beat()
		}
	}

	pace := func(took time.Duration) {
		if opts.Pace {
			time.Sleep(min(took, maxReadPause))
		}
	}

	for _, e := range s.entries {
		if e.measured {
			continue
		}

		sum, size, runErr := e.run(nil, func() { beat(opts.Flush != nil || opts.Beat != nil) }, pace)
		if runErr != nil {
			return runErr
		}

		e.size = size
		s.manifest.Files[e.name] = sum
	}

	tw := tar.NewWriter(gz)
	modTime := s.now.UTC().Truncate(time.Second)

	manifestData, err := json.MarshalIndent(s.manifest, "", "  ")
	if err != nil {
		return err
	}

	manifestData = append(manifestData, '\n')
	if err := writeEntryHeader(tw, ManifestPath, int64(len(manifestData)), modTime); err != nil {
		return err
	}

	if _, err := tw.Write(manifestData); err != nil {
		return err
	}

	for _, e := range s.entries {
		if err := writeEntryHeader(tw, e.name, e.size, modTime); err != nil {
			return err
		}

		sum, _, runErr := e.run(tw, func() { beat(false) }, pace)
		if runErr != nil {
			return runErr
		}

		if want := s.manifest.Files[e.name]; sum != want {
			return fmt.Errorf("%s changed while the backup was read", e.name)
		}
	}

	if err := tw.Close(); err != nil {
		return err
	}

	return gz.Close()
}

func writeEntryHeader(tw *tar.Writer, name string, size int64, modTime time.Time) error {
	return tw.WriteHeader(&tar.Header{
		Name:     name,
		Mode:     0o600,
		Size:     size,
		ModTime:  modTime,
		Typeflag: tar.TypeReg,
	})
}

func (s *Source) spool(now time.Time) error {
	folder, err := os.MkdirTemp("", "elysiandb-backup-")
	if err != nil {
		return err
	}

	previous := s.release
	s.release = func() {
		previous()
		os.RemoveAll(folder)
	}

	add := func(name string, write func(w io.Writer) error) error {
		file, err := os.CreateTemp(folder, "entry-*")
		if err != nil {
			return err
		}

		hash := sha256.New()
		buffered := bufio.NewWriter(io.MultiWriter(file, hash))

		err = write(buffered)
		if err == nil {
			err = buffered.Flush()
		}

		if closeErr := file.Close(); err == nil {
			err = closeErr
		}

		if err != nil {
			return err
		}

		info, err := os.Stat(file.Name())
		if err != nil {
			return err
		}

		path := file.Name()
		s.manifest.Files[name] = hex.EncodeToString(hash.Sum(nil))
		s.entries = append(s.entries, &entry{name: name, size: info.Size(), measured: true, write: func(w io.Writer) error {
			f, err := os.Open(path)
			if err != nil {
				return err
			}
			defer f.Close()

			_, err = io.Copy(w, f)

			return err
		}})

		return nil
	}

	key := s.entries[0]
	s.entries = nil
	if err := add(key.name, key.write); err != nil {
		return err
	}

	types := engine.ListEntityTypes()
	if err := engine.IterateEntities(schema.SchemaEntity, func(data map[string]any) error {
		entity, _ := data["id"].(string)
		if entity == "" {
			return nil
		}

		s.manifest.Schemas = append(s.manifest.Schemas, entity)

		return add(SchemaPath(entity), func(w io.Writer) error {
			return newEncoder(w).Encode(data)
		})
	}); err != nil {
		return err
	}

	seen := map[string]bool{}
	for _, entity := range types {
		if entity == schema.SchemaEntity || seen[entity] {
			continue
		}

		seen[entity] = true
		count := 0

		if err := add(EntityPath(entity), func(w io.Writer) error {
			encoder := newEncoder(w)
			return engine.IterateEntities(entity, func(data map[string]any) error {
				if id, _ := data["id"].(string); id == "" {
					return nil
				}

				count++
				return encoder.Encode(data)
			})
		}); err != nil {
			return err
		}

		s.manifest.Entities = append(s.manifest.Entities, EntityManifest{Name: entity, Documents: count})
	}

	return add(KVPath, func(w io.Writer) error {
		encoder := newEncoder(w)
		for _, key := range storage.ListKeys() {
			if globals.IsApiKey(key) || storage.KeyHasExpired(key) {
				continue
			}

			value, err := storage.GetByKey(key)
			if err != nil {
				continue
			}

			kv := newKVEntry(key, value)
			if expiresAt, ok := storage.GetKeyExpiration(key); ok {
				kv.ExpiresAt = expiresAt
			}

			if err := encoder.Encode(kv); err != nil {
				return err
			}

			s.manifest.KVKeys++
		}

		return nil
	})
}
