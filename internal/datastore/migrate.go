package datastore

import (
	"bytes"
	"encoding/json"
	"fmt"
	"github.com/taymour/elysiandb/internal/durable"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/journal"
	"github.com/taymour/elysiandb/internal/legacy"
	"github.com/taymour/elysiandb/internal/log"
)

const (
	LegacyFolder    = "legacy-0.2.4"
	migrationFolder = ".migration"
	migrationBatch  = 1000
)

type MigrationReport struct {
	Entities  int
	Documents int
	KVKeys    int
	Replayed  int
	Duration  time.Duration
}

func (e *Engine) migrateIfNeeded() (*MigrationReport, error) {
	stored, err := e.manifests.Stored()
	if err != nil {
		return nil, err
	}

	if len(stored) > 0 {
		e.moveLegacyFiles()
		return nil, nil
	}

	if !legacy.Present(e.dir) {
		return nil, nil
	}

	files := e.files()
	start := time.Now()
	if err := removeTree(files, filepath.Join(e.dir, LogFolder)); err != nil {
		return nil, err
	}
	if err := removeTree(files, filepath.Join(e.dir, durable.PagesFolder)); err != nil {
		return nil, err
	}
	tmp := filepath.Join(e.dir, migrationFolder)
	if err := removeTree(files, tmp); err != nil {
		return nil, err
	}

	if err := checkFreeSpace(e.dir); err != nil {
		return nil, err
	}

	engineLog.Info("migrating a 0.2.4 data folder, the first start takes longer", "bytes", legacyBytes(e.dir), "estimated_seconds", legacyBytes(e.dir)/migrationBytesPerSecond+1)

	data, err := legacy.Read(e.dir, e.opts.LegacyRecovery, time.Now())
	if err != nil {
		return nil, fmt.Errorf("cannot read the 0.2.4 data files: %w", err)
	}

	report, err := writeMigration(tmp, data, e.opts.Shards, e.opts.FS)
	if err != nil {
		_ = removeTree(files, tmp)
		return nil, fmt.Errorf("migration failed, the 0.2.4 files are untouched: %w", err)
	}

	if err := verifyMigration(tmp, data, e.opts.Shards, e.opts.FS); err != nil {
		_ = removeTree(files, tmp)
		return nil, fmt.Errorf("migration check failed, the 0.2.4 files are untouched: %w", err)
	}

	entries, err := files.ReadDir(tmp)
	if err != nil {
		return nil, err
	}

	for _, entry := range entries {
		name := entry.Name()
		if name == ManifestFile || name == LockFile {
			continue
		}

		if err := files.Rename(filepath.Join(tmp, name), filepath.Join(e.dir, name)); err != nil {
			return nil, err
		}
	}

	if err := files.SyncDir(e.dir); err != nil {
		return nil, err
	}

	if err := files.Rename(filepath.Join(tmp, ManifestFile), filepath.Join(e.dir, ManifestFile)); err != nil {
		return nil, err
	}

	if err := files.SyncDir(e.dir); err != nil {
		return nil, err
	}

	if err := removeTree(files, tmp); err != nil {
		engineLog.Warn("cannot remove the migration folder", "err", err)
	}
	e.moveLegacyFiles()

	report.Replayed = data.Replayed
	report.Duration = time.Since(start)
	engineLog.Info("storage migrated",
		"from", "0.2.4",
		"entity_types", report.Entities,
		"documents", report.Documents,
		"kv_keys", report.KVKeys,
		"recovery_entries_replayed", report.Replayed,
		"legacy_files", filepath.Join(e.dir, LegacyFolder),
		"duration_ms", log.Since(start),
	)

	return report, nil
}

const migrationBytesPerSecond = 32 << 20

func removeTree(files durable.FS, path string) error {
	entries, err := files.ReadDir(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}

	for _, entry := range entries {
		child := filepath.Join(path, entry.Name())
		if entry.IsDir() {
			if err := removeTree(files, child); err != nil {
				return err
			}
			continue
		}

		if err := files.Remove(child); err != nil && !os.IsNotExist(err) {
			return err
		}
	}

	if err := files.Remove(path); err != nil && !os.IsNotExist(err) {
		return err
	}

	return nil
}

func legacyBytes(dir string) int64 {
	var used int64
	for _, name := range legacy.FilesIn(dir) {
		if info, err := os.Stat(filepath.Join(dir, name)); err == nil {
			used += info.Size()
		}
	}

	return used
}

func (e *Engine) moveLegacyFiles() {
	files := legacy.FilesIn(e.dir)
	if len(files) == 0 {
		return
	}

	target := filepath.Join(e.dir, LegacyFolder)
	if err := os.MkdirAll(target, 0o700); err != nil {
		engineLog.Warn("cannot keep the 0.2.4 files aside", "err", err)
		return
	}

	for _, name := range files {
		dst := filepath.Join(target, name)
		if _, err := os.Stat(dst); err == nil {
			dst = filepath.Join(target, name+"."+time.Now().UTC().Format("20060102T150405"))
		}

		if err := os.Rename(filepath.Join(e.dir, name), dst); err != nil {
			engineLog.Warn("cannot keep a 0.2.4 file aside", "file", name, "err", err)
			continue
		}

		_ = os.Chmod(dst, 0o600)
	}
}

func checkFreeSpace(dir string) error {
	used := legacyBytes(dir)

	free, err := durable.FreeBytes(dir)
	if err != nil {
		return nil
	}

	if free < 2*used {
		return fmt.Errorf("not enough free space to migrate the data folder: %d bytes free, %d needed", free, 2*used)
	}

	return nil
}

func writeMigration(dir string, data *legacy.Data, shards int, files durable.FS) (*MigrationReport, error) {
	e, err := Open(Options{Folder: dir, Shards: shards, Durability: journal.ModeWrite, FS: files})
	if err != nil {
		return nil, err
	}

	report := &MigrationReport{}
	writeErr := func() error {
		if err := e.Update(func(tx *Tx) error {
			for _, entity := range data.Types {
				tx.EnsureType(entity)
			}
			return nil
		}); err != nil {
			return err
		}

		for _, entity := range data.Types {
			docs := data.Entities[entity]
			report.Entities++
			for start := 0; start < len(docs); start += migrationBatch {
				end := min(start+migrationBatch, len(docs))
				if err := e.Update(func(tx *Tx) error {
					for _, doc := range docs[start:end] {
						if err := tx.Put(entity, doc.ID, doc.Doc); err != nil {
							return err
						}
					}
					return nil
				}); err != nil {
					return err
				}
				report.Documents += end - start
			}
		}

		for key, value := range data.KV {
			if strings.HasPrefix(key, legacy.ApiKeyPrefix) {
				continue
			}

			if err := e.KVPut(key, value, data.Expirations[key]); err != nil {
				return err
			}
			report.KVKeys++
		}

		return nil
	}()

	if writeErr != nil {
		e.Abandon()
		return nil, writeErr
	}

	if err := e.Close(); err != nil {
		return nil, err
	}

	return report, nil
}

func verifyMigration(dir string, data *legacy.Data, shards int, files durable.FS) error {
	e, err := Open(Options{Folder: dir, Shards: shards, Durability: journal.ModeWrite, FS: files})
	if err != nil {
		return err
	}
	defer e.Abandon()

	types := e.Types()
	if len(types) != len(data.Types) {
		return fmt.Errorf("%d entity types migrated, %d expected", len(types), len(data.Types))
	}

	for i, t := range data.Types {
		if types[i] != t {
			return fmt.Errorf("entity type #%d is %q, %q expected", i, types[i], t)
		}

		snap := e.Snapshot(t)
		docs := data.Entities[t]
		if snap.Len() != len(docs) {
			return fmt.Errorf("%s holds %d documents, %d expected", t, snap.Len(), len(docs))
		}

		i := 0
		var mismatch error
		snap.Ascend(0, func(d Doc) bool {
			want, _ := json.Marshal(docs[i].Doc)
			got, err := d.Bytes()
			if err != nil || d.ID() != docs[i].ID || !bytes.Equal(got, want) {
				mismatch = fmt.Errorf("%s document #%d (%s) differs after migration", t, i, docs[i].ID)
				return false
			}
			i++
			return true
		})

		if mismatch != nil {
			return mismatch
		}
	}

	for key, value := range data.KV {
		if strings.HasPrefix(key, legacy.ApiKeyPrefix) {
			continue
		}

		got, exp, ok := e.KVGet(key)
		if !ok || !bytes.Equal(got, value) || exp != data.Expirations[key] {
			return fmt.Errorf("key %q differs after migration", key)
		}
	}

	return nil
}

func (e *Engine) ExportLegacy(folder string) (*legacy.Data, error) {
	snaps := e.SnapshotAll()
	data := &legacy.Data{KV: map[string][]byte{}, Expirations: map[string]int64{}, Types: e.Types(), Entities: map[string][]legacy.Document{}}

	for _, t := range data.Types {
		snap, ok := snaps[t]
		if !ok {
			continue
		}

		var decodeErr error
		snap.Ascend(0, func(d Doc) bool {
			doc, err := d.Decode()
			if err != nil {
				decodeErr = err
				return false
			}

			data.Entities[t] = append(data.Entities[t], legacy.Document{ID: d.ID(), Doc: doc})
			return true
		})

		if decodeErr != nil {
			return nil, decodeErr
		}
	}

	for _, key := range e.KVKeys() {
		value, exp, ok := e.KVGet(key)
		if !ok {
			continue
		}

		data.KV[key] = value
		if exp > 0 {
			data.Expirations[key] = exp
		}
	}

	if err := legacy.Write(folder, data); err != nil {
		return nil, err
	}

	return data, nil
}
