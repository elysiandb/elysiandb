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
	"time"
	"unicode/utf8"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

type spooledFile struct {
	name string
	path string
	size int64
}

type spool struct {
	folder string
	files  []spooledFile
	sums   map[string]string
}

func Create(w io.Writer, now time.Time) (*Manifest, error) {
	folder, err := os.MkdirTemp("", "elysiandb-backup-")
	if err != nil {
		return nil, err
	}

	defer os.RemoveAll(folder)

	s := &spool{folder: folder, sums: map[string]string{}}
	manifest, err := collect(s, now)
	if err != nil {
		return nil, err
	}

	if err := writeArchive(w, manifest, s.files, now); err != nil {
		return nil, err
	}

	return manifest, nil
}

func WriteFile(folder string, now time.Time) (string, *Manifest, error) {
	path := filepath.Join(folder, FileName(now))
	if _, err := os.Stat(path); err == nil {
		return "", nil, fmt.Errorf("%s already exists", path)
	}

	tmp, err := os.CreateTemp(folder, "."+FileName(now)+".*")
	if err != nil {
		return "", nil, err
	}

	defer os.Remove(tmp.Name())

	manifest, err := Create(tmp, now)
	if closeErr := tmp.Close(); err == nil {
		err = closeErr
	}

	if err != nil {
		return "", nil, err
	}

	if err := os.Rename(tmp.Name(), path); err != nil {
		return "", nil, err
	}

	return path, manifest, nil
}

func collect(s *spool, now time.Time) (*Manifest, error) {
	manifest := &Manifest{
		Format:           Format,
		FormatVersion:    FormatVersion,
		ElysianDBVersion: globals.VERSION,
		CreatedAt:        now.UTC().Format(time.RFC3339),
		Engine:           globals.GetEngine(),
		Entities:         []EntityManifest{},
		Schemas:          []string{},
	}

	key, err := security.CreateKeyFileOrGetKey()
	if err != nil {
		return nil, err
	}

	err = s.add(KeyPath, func(w io.Writer) error {
		_, err := io.WriteString(w, key)
		return err
	})
	if err != nil {
		return nil, err
	}

	if err := collectSchemas(s, manifest); err != nil {
		return nil, err
	}

	if err := collectEntities(s, manifest); err != nil {
		return nil, err
	}

	if err := collectKeys(s, manifest); err != nil {
		return nil, err
	}

	manifest.Files = s.sums

	return manifest, nil
}

func collectSchemas(s *spool, manifest *Manifest) error {
	return engine.IterateEntities(schema.SchemaEntity, func(data map[string]any) error {
		entity, _ := data["id"].(string)
		if entity == "" {
			return nil
		}

		manifest.Schemas = append(manifest.Schemas, entity)

		return s.add(SchemaPath(entity), func(w io.Writer) error {
			return newEncoder(w).Encode(data)
		})
	})
}

func collectEntities(s *spool, manifest *Manifest) error {
	seen := map[string]bool{}
	for _, entity := range engine.ListEntityTypes() {
		if entity == schema.SchemaEntity || seen[entity] {
			continue
		}

		seen[entity] = true
		count := 0

		err := s.add(EntityPath(entity), func(w io.Writer) error {
			encoder := newEncoder(w)
			return engine.IterateEntities(entity, func(data map[string]any) error {
				if id, _ := data["id"].(string); id == "" {
					return nil
				}

				count++
				return encoder.Encode(data)
			})
		})
		if err != nil {
			return err
		}

		manifest.Entities = append(manifest.Entities, EntityManifest{Name: entity, Documents: count})
	}

	return nil
}

func collectKeys(s *spool, manifest *Manifest) error {
	return s.add(KVPath, func(w io.Writer) error {
		encoder := newEncoder(w)
		for _, key := range storage.ListKeys() {
			if globals.IsApiKey(key) || storage.KeyHasExpired(key) {
				continue
			}

			value, err := storage.GetByKey(key)
			if err != nil {
				continue
			}

			entry := newKVEntry(key, value)
			if expiresAt, ok := storage.GetKeyExpiration(key); ok {
				entry.ExpiresAt = expiresAt
			}

			if err := encoder.Encode(entry); err != nil {
				return err
			}

			manifest.KVKeys++
		}

		return nil
	})
}

func newKVEntry(key string, value []byte) KVEntry {
	if utf8.Valid(value) {
		return KVEntry{Key: key, Value: string(value)}
	}

	return KVEntry{Key: key, Value: base64.StdEncoding.EncodeToString(value), Encoding: KVEncodingBase64}
}

func (s *spool) add(name string, write func(w io.Writer) error) error {
	file, err := os.CreateTemp(s.folder, "entry-*")
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

	s.files = append(s.files, spooledFile{name: name, path: file.Name(), size: info.Size()})
	s.sums[name] = hex.EncodeToString(hash.Sum(nil))

	return nil
}

func writeArchive(w io.Writer, manifest *Manifest, files []spooledFile, now time.Time) error {
	modTime := now.UTC().Truncate(time.Second)
	gz := gzip.NewWriter(w)
	tw := tar.NewWriter(gz)

	manifestData, err := json.MarshalIndent(manifest, "", "  ")
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

	for _, f := range files {
		if err := copyEntry(tw, f, modTime); err != nil {
			return err
		}
	}

	if err := tw.Close(); err != nil {
		return err
	}

	return gz.Close()
}

func copyEntry(tw *tar.Writer, f spooledFile, modTime time.Time) error {
	file, err := os.Open(f.path)
	if err != nil {
		return err
	}

	defer file.Close()

	if err := writeEntryHeader(tw, f.name, f.size, modTime); err != nil {
		return err
	}

	_, err = io.Copy(tw, file)

	return err
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
