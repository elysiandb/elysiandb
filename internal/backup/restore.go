package backup

import (
	"archive/tar"
	"compress/gzip"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

const (
	maxManifestSize = 64 << 20
	maxKeySize      = 64 << 10
)

type Report struct {
	Entities      map[string]int `json:"entities"`
	Schemas       int            `json:"schemas"`
	KVKeys        int            `json:"kv_keys"`
	ExpiredKVKeys int            `json:"kv_keys_expired"`
}

type archiveReader struct {
	tr *tar.Reader
}

func Restore(src io.ReadSeeker) (*Report, error) {
	manifest, err := Verify(src)
	if err != nil {
		return nil, err
	}

	if _, err := src.Seek(0, io.SeekStart); err != nil {
		return nil, err
	}

	return apply(src, manifest)
}

func Verify(src io.Reader) (*Manifest, error) {
	archive, err := openArchive(src)
	if err != nil {
		return nil, err
	}

	manifest, err := archive.readManifest()
	if err != nil {
		return nil, err
	}

	for _, path := range manifest.paths() {
		r, err := archive.next(path)
		if err != nil {
			return nil, err
		}

		if err := verifyEntry(r, path, manifest.Files[path], manifest.checkerFor(path)); err != nil {
			return nil, err
		}
	}

	if err := archive.end(); err != nil {
		return nil, err
	}

	return manifest, nil
}

func (m *Manifest) checkerFor(path string) func(io.Reader) error {
	if path == KeyPath {
		return func(r io.Reader) error {
			_, err := readKey(r)
			return err
		}
	}

	if path == KVPath {
		return func(r io.Reader) error {
			return expectCount(path, m.KVKeys, newKVReader(r, path).count)
		}
	}

	for _, entity := range m.Schemas {
		if SchemaPath(entity) == path {
			return func(r io.Reader) error {
				_, err := readSchema(r, path, entity)
				return err
			}
		}
	}

	for _, entity := range m.Entities {
		if EntityPath(entity.Name) == path {
			return func(r io.Reader) error {
				return expectCount(path, entity.Documents, newDocumentReader(r, path).count)
			}
		}
	}

	return func(io.Reader) error {
		return invalidArchive("unexpected file %s", path)
	}
}

func expectCount(path string, expected int, count func() (int, error)) error {
	n, err := count()
	if err != nil {
		return err
	}

	if n != expected {
		return invalidArchive("%s holds %d entries, the manifest announces %d", path, n, expected)
	}

	return nil
}

func verifyEntry(r io.Reader, path, expectedSum string, check func(io.Reader) error) error {
	hash := sha256.New()
	tee := io.TeeReader(r, hash)

	checkErr := check(tee)
	if _, err := io.Copy(io.Discard, tee); err != nil {
		return invalidArchive("%s cannot be read: %v", path, err)
	}

	if hex.EncodeToString(hash.Sum(nil)) != expectedSum {
		return invalidArchive("checksum mismatch for %s", path)
	}

	return checkErr
}

func apply(src io.Reader, manifest *Manifest) (*Report, error) {
	archive, err := openArchive(src)
	if err != nil {
		return nil, err
	}

	if _, err := archive.next(ManifestPath); err != nil {
		return nil, err
	}

	r, err := archive.next(KeyPath)
	if err != nil {
		return nil, err
	}

	key, err := readKey(r)
	if err != nil {
		return nil, err
	}

	if err := security.WriteKey(key); err != nil {
		return nil, err
	}

	report, err := replaceData(archive, manifest)
	if err != nil {
		return nil, &IncompleteRestoreError{Err: err}
	}

	return report, nil
}

func replaceData(archive *archiveReader, manifest *Manifest) (*Report, error) {
	if err := engine.WipeAll(); err != nil {
		return nil, err
	}

	report := &Report{Entities: map[string]int{}}

	schemas, err := restoreSchemas(archive, manifest)
	if err != nil {
		return nil, err
	}

	report.Schemas = schemas

	for _, entity := range manifest.Entities {
		path := EntityPath(entity.Name)
		r, err := archive.next(path)
		if err != nil {
			return nil, err
		}

		count, err := engine.RestoreEntities(entity.Name, newDocumentReader(r, path).next)
		if err != nil {
			return nil, err
		}

		report.Entities[entity.Name] = count
	}

	if err := restoreKeys(archive, report); err != nil {
		return nil, err
	}

	if err := finishRestore(); err != nil {
		return nil, err
	}

	return report, nil
}

func restoreSchemas(archive *archiveReader, manifest *Manifest) (int, error) {
	schemas := make([]map[string]any, 0, len(manifest.Schemas))
	for _, entity := range manifest.Schemas {
		path := SchemaPath(entity)
		r, err := archive.next(path)
		if err != nil {
			return 0, err
		}

		data, err := readSchema(r, path, entity)
		if err != nil {
			return 0, err
		}

		schemas = append(schemas, data)
	}

	return engine.RestoreEntities(schema.SchemaEntity, func() (map[string]any, error) {
		if len(schemas) == 0 {
			return nil, io.EOF
		}

		data := schemas[0]
		schemas = schemas[1:]

		return data, nil
	})
}

func restoreKeys(archive *archiveReader, report *Report) error {
	r, err := archive.next(KVPath)
	if err != nil {
		return err
	}

	keys := newKVReader(r, KVPath)
	now := time.Now().Unix()
	for {
		entry, value, err := keys.next()
		if err == io.EOF {
			return nil
		}

		if err != nil {
			return err
		}

		if entry.ExpiresAt > 0 && entry.ExpiresAt <= now {
			report.ExpiredKVKeys++
			continue
		}

		if err := storage.PutKeyValueWithExpiration(entry.Key, value, entry.ExpiresAt); err != nil {
			return err
		}

		report.KVKeys++
	}
}

func finishRestore() error {
	if err := security.DeleteAllSessions(); err != nil {
		return err
	}

	if globals.GetConfig().Api.Cache.Enabled && cache.CacheStore != nil {
		cache.CacheStore.PurgeAll()
	}

	if err := security.InitAdminUserIfNotExists(); err != nil {
		return err
	}

	if err := acl.InitACL(); err != nil {
		return err
	}

	hook.InitHooks()

	if engine.IsEngineInternal() {
		api_storage.RebuildAllIndexes()
	}

	storage.WriteToDB()

	return nil
}

func openArchive(src io.Reader) (*archiveReader, error) {
	gz, err := gzip.NewReader(src)
	if err != nil {
		return nil, invalidArchive("not a gzip file: %v", err)
	}

	return &archiveReader{tr: tar.NewReader(gz)}, nil
}

func (a *archiveReader) next(expected string) (io.Reader, error) {
	header, err := a.tr.Next()
	if err == io.EOF {
		return nil, invalidArchive("%s is missing", expected)
	}

	if err != nil {
		return nil, invalidArchive("unreadable tar stream: %v", err)
	}

	if header.Name != expected {
		return nil, invalidArchive("expected %s, found %s", expected, header.Name)
	}

	if header.Typeflag != tar.TypeReg {
		return nil, invalidArchive("%s is not a regular file", expected)
	}

	return a.tr, nil
}

func (a *archiveReader) end() error {
	header, err := a.tr.Next()
	if err == io.EOF {
		return nil
	}

	if err != nil {
		return invalidArchive("unreadable tar stream: %v", err)
	}

	return invalidArchive("unexpected file %s", header.Name)
}

func (a *archiveReader) readManifest() (*Manifest, error) {
	r, err := a.next(ManifestPath)
	if err != nil {
		return nil, err
	}

	data, err := io.ReadAll(io.LimitReader(r, maxManifestSize+1))
	if err != nil {
		return nil, invalidArchive("%s cannot be read: %v", ManifestPath, err)
	}

	if len(data) > maxManifestSize {
		return nil, invalidArchive("%s is too large", ManifestPath)
	}

	var manifest Manifest
	if err := json.Unmarshal(data, &manifest); err != nil {
		return nil, invalidArchive("%s is not valid JSON: %v", ManifestPath, err)
	}

	if err := manifest.validate(); err != nil {
		return nil, err
	}

	return &manifest, nil
}

func (m *Manifest) validate() error {
	if m.Format != Format {
		return invalidArchive("unknown format %q", m.Format)
	}

	if m.FormatVersion < 1 {
		return invalidArchive("invalid format version %d", m.FormatVersion)
	}

	if m.FormatVersion > FormatVersion {
		return invalidArchive("format version %d is newer than the version %d supported by ElysianDB %s", m.FormatVersion, FormatVersion, globals.VERSION)
	}

	if m.KVKeys < 0 {
		return invalidArchive("invalid KV key count %d", m.KVKeys)
	}

	for _, entity := range m.Entities {
		if entity.Name == "" || entity.Documents < 0 {
			return invalidArchive("invalid entity entry %+v", entity)
		}
	}

	paths := m.paths()
	unique := map[string]bool{}
	for _, path := range paths {
		if unique[path] {
			return invalidArchive("%s is listed twice", path)
		}

		unique[path] = true
		if _, ok := m.Files[path]; !ok {
			return invalidArchive("no checksum for %s", path)
		}
	}

	if len(m.Files) != len(paths) {
		for path := range m.Files {
			if !unique[path] {
				return invalidArchive("checksum for an unknown file %s", path)
			}
		}
	}

	return nil
}

func readKey(r io.Reader) (string, error) {
	data, err := io.ReadAll(io.LimitReader(r, maxKeySize+1))
	if err != nil {
		return "", invalidArchive("%s cannot be read: %v", KeyPath, err)
	}

	if len(data) > maxKeySize {
		return "", invalidArchive("%s is too large", KeyPath)
	}

	key := strings.TrimSpace(string(data))
	if key == "" || strings.ContainsAny(key, " \t\r\n") {
		return "", invalidArchive("%s does not hold a valid key", KeyPath)
	}

	return key, nil
}

func readSchema(r io.Reader, path, entity string) (map[string]any, error) {
	decoder := json.NewDecoder(r)

	var data map[string]any
	if err := decoder.Decode(&data); err != nil || data == nil {
		return nil, invalidArchive("%s is not a JSON object", path)
	}

	if id, _ := data["id"].(string); id != entity {
		return nil, invalidArchive("%s describes %q instead of %q", path, data["id"], entity)
	}

	if fields, ok := data["fields"]; ok {
		if _, ok := fields.(map[string]any); !ok {
			return nil, invalidArchive("%s has invalid fields", path)
		}
	}

	if err := expectEnd(decoder); err != nil {
		return nil, invalidArchive("%s holds more than one schema", path)
	}

	return data, nil
}

func expectEnd(decoder *json.Decoder) error {
	var extra any
	if err := decoder.Decode(&extra); err != io.EOF {
		return errors.New("unexpected data")
	}

	return nil
}

type documentReader struct {
	decoder *json.Decoder
	path    string
	line    int
	ids     map[string]bool
}

func newDocumentReader(r io.Reader, path string) *documentReader {
	return &documentReader{decoder: json.NewDecoder(r), path: path, ids: map[string]bool{}}
}

func (d *documentReader) next() (map[string]any, error) {
	var data map[string]any
	if err := d.decoder.Decode(&data); err != nil {
		if err == io.EOF {
			return nil, io.EOF
		}

		return nil, invalidArchive("%s, document %d: %v", d.path, d.line+1, err)
	}

	d.line++

	id, _ := data["id"].(string)
	if data == nil || id == "" {
		return nil, invalidArchive("%s, document %d: a document must be an object with a string id", d.path, d.line)
	}

	if d.ids[id] {
		return nil, invalidArchive("%s, document %d: duplicate id %q", d.path, d.line, id)
	}

	d.ids[id] = true

	return data, nil
}

func (d *documentReader) count() (int, error) {
	for {
		if _, err := d.next(); err != nil {
			if err == io.EOF {
				return d.line, nil
			}

			return 0, err
		}
	}
}

type kvReader struct {
	decoder *json.Decoder
	path    string
	line    int
	keys    map[string]bool
}

func newKVReader(r io.Reader, path string) *kvReader {
	decoder := json.NewDecoder(r)
	decoder.DisallowUnknownFields()

	return &kvReader{decoder: decoder, path: path, keys: map[string]bool{}}
}

func (k *kvReader) next() (KVEntry, []byte, error) {
	var entry KVEntry
	if err := k.decoder.Decode(&entry); err != nil {
		if err == io.EOF {
			return entry, nil, io.EOF
		}

		return entry, nil, invalidArchive("%s, entry %d: %v", k.path, k.line+1, err)
	}

	k.line++

	if entry.Key == "" || globals.IsApiKey(entry.Key) {
		return entry, nil, invalidArchive("%s, entry %d: invalid key %q", k.path, k.line, entry.Key)
	}

	if k.keys[entry.Key] {
		return entry, nil, invalidArchive("%s, entry %d: duplicate key %q", k.path, k.line, entry.Key)
	}

	k.keys[entry.Key] = true

	if entry.ExpiresAt < 0 {
		return entry, nil, invalidArchive("%s, entry %d: invalid expiration %d", k.path, k.line, entry.ExpiresAt)
	}

	switch entry.Encoding {
	case "":
		return entry, []byte(entry.Value), nil
	case KVEncodingBase64:
		value, err := base64.StdEncoding.DecodeString(entry.Value)
		if err != nil {
			return entry, nil, invalidArchive("%s, entry %d: invalid base64 value", k.path, k.line)
		}

		return entry, value, nil
	default:
		return entry, nil, invalidArchive("%s, entry %d: unknown encoding %q", k.path, k.line, entry.Encoding)
	}
}

func (k *kvReader) count() (int, error) {
	for {
		if _, _, err := k.next(); err != nil {
			if err == io.EOF {
				return k.line, nil
			}

			return 0, err
		}
	}
}
