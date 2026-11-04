package datastore

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"syscall"
	"time"

	"github.com/taymour/elysiandb/internal/legacy"
	"github.com/taymour/elysiandb/internal/legacy/logengine"
)

const (
	LegacyFolder    = "legacy-0.2.4"
	LegacyLogFolder = "legacy-log-pages"
	migrationFolder = ".migration"
	migrationBatch  = 1000
	migrationCache  = 64 << 20
)

type MigrationReport struct {
	From      string
	Entities  int
	Documents int
	KVKeys    int
	Replayed  int
	Duration  time.Duration
}

type migrationDoc struct {
	id    string
	value []byte
}

type migrationSource interface {
	types() []string
	entities() []string
	refs(entity string) []RefPath
	each(entity string, fn func(doc migrationDoc) error) error
	kv(fn func(key string, value []byte, expiresAt int64) error) error
	usedBytes() int64
}

func (e *Engine) migrateIfNeeded() (*MigrationReport, error) {
	if _, err := os.Stat(filepath.Join(e.dir, PebbleFolder)); err == nil {
		e.moveAside(LegacyLogFolder, logengine.FilesIn(e.dir))
		e.moveAside(LegacyFolder, legacy.FilesIn(e.dir))
		return nil, nil
	}

	switch {
	case logengine.Present(e.dir):
		folder, err := logengine.Open(e.dir, time.Now())
		if err != nil {
			return nil, fmt.Errorf("cannot read the data files of the previous engine: %w", err)
		}
		defer folder.Close()

		src := &logSource{dir: e.dir, f: folder}
		report, err := e.migrate(src, "log-pages")
		if err != nil {
			return nil, err
		}
		report.Replayed = folder.Replayed()
		e.moveAside(LegacyLogFolder, logengine.FilesIn(e.dir))
		e.moveAside(LegacyFolder, legacy.FilesIn(e.dir))
		e.logMigration(report, LegacyLogFolder)

		return report, nil
	case legacy.Present(e.dir):
		data, err := legacy.Read(e.dir, e.opts.LegacyRecovery, time.Now())
		if err != nil {
			return nil, fmt.Errorf("cannot read the 0.2.4 data files: %w", err)
		}

		src := &legacySource{dir: e.dir, data: data}
		report, err := e.migrate(src, "0.2.4")
		if err != nil {
			return nil, err
		}
		report.Replayed = data.Replayed
		e.moveAside(LegacyFolder, legacy.FilesIn(e.dir))
		e.logMigration(report, LegacyFolder)

		return report, nil
	}

	return nil, nil
}

func (e *Engine) logMigration(report *MigrationReport, kept string) {
	engineLog.Info("storage migrated",
		"from", report.From,
		"entity_types", report.Entities,
		"documents", report.Documents,
		"kv_keys", report.KVKeys,
		"recovery_entries_replayed", report.Replayed,
		"legacy_files", filepath.Join(e.dir, kept),
		"duration_ms", float64(report.Duration.Microseconds())/1000,
	)
}

func (e *Engine) migrate(src migrationSource, from string) (*MigrationReport, error) {
	start := time.Now()
	tmp := filepath.Join(e.dir, migrationFolder)
	_ = os.RemoveAll(tmp)

	if err := checkFreeSpace(e.dir, src.usedBytes()); err != nil {
		return nil, err
	}

	report, err := writeMigration(tmp, e.opts.Shards, src)
	if err != nil {
		_ = os.RemoveAll(tmp)
		return nil, fmt.Errorf("migration failed, the previous files are untouched: %w", err)
	}

	if err := verifyMigration(tmp, e.opts.Shards, src); err != nil {
		_ = os.RemoveAll(tmp)
		return nil, fmt.Errorf("migration check failed, the previous files are untouched: %w", err)
	}

	if err := os.Rename(filepath.Join(tmp, PebbleFolder), filepath.Join(e.dir, PebbleFolder)); err != nil {
		_ = os.RemoveAll(tmp)
		return nil, err
	}

	_ = syncDir(e.dir)
	_ = os.RemoveAll(tmp)

	report.From = from
	report.Duration = time.Since(start)

	return report, nil
}

func syncDir(dir string) error {
	d, err := os.Open(dir)
	if err != nil {
		return err
	}
	defer d.Close()

	return d.Sync()
}

func (e *Engine) moveAside(folder string, files []string) {
	if len(files) == 0 {
		return
	}

	target := filepath.Join(e.dir, folder)
	if err := os.MkdirAll(target, 0o700); err != nil {
		engineLog.Warn("cannot keep the previous data files aside", "err", err)
		return
	}

	for _, name := range files {
		dst := filepath.Join(target, name)
		if _, err := os.Stat(dst); err == nil {
			dst = filepath.Join(target, name+"."+time.Now().UTC().Format("20060102T150405"))
		}

		if err := os.Rename(filepath.Join(e.dir, name), dst); err != nil {
			engineLog.Warn("cannot keep a previous data file aside", "file", name, "err", err)
			continue
		}

		if info, err := os.Stat(dst); err == nil && !info.IsDir() {
			_ = os.Chmod(dst, 0o600)
		}
	}

	_ = syncDir(e.dir)
}

func checkFreeSpace(dir string, used int64) error {
	var st syscall.Statfs_t
	if err := syscall.Statfs(dir, &st); err != nil {
		return nil
	}

	free := int64(st.Bavail) * int64(st.Bsize)
	if free < 2*used {
		return fmt.Errorf("not enough free space to migrate the data folder: %d bytes free, %d needed", free, 2*used)
	}

	return nil
}

func migrationOptions(dir string, shards int) Options {
	return Options{Folder: dir, Shards: shards, Durability: DurabilityBackground, CacheBytes: migrationCache, MemTableBytes: migrationCache}
}

func writeMigration(dir string, shards int, src migrationSource) (*MigrationReport, error) {
	m, err := Open(migrationOptions(dir, shards))
	if err != nil {
		return nil, err
	}

	report := &MigrationReport{}
	writeErr := func() error {
		types := src.types()
		if err := m.Update(func(tx *Tx) error {
			for _, entity := range types {
				tx.EnsureType(entity)
			}
			return nil
		}); err != nil {
			return err
		}
		report.Entities = len(types)

		for _, entity := range src.entities() {
			batch := make([]migrationDoc, 0, migrationBatch)
			flush := func() error {
				if len(batch) == 0 {
					return nil
				}

				err := m.Update(func(tx *Tx) error {
					for _, doc := range batch {
						if err := tx.PutBytes(entity, doc.id, doc.value, nil); err != nil {
							return err
						}
					}
					return nil
				})
				report.Documents += len(batch)
				batch = batch[:0]

				return err
			}

			if err := src.each(entity, func(doc migrationDoc) error {
				batch = append(batch, doc)
				if len(batch) >= migrationBatch {
					return flush()
				}
				return nil
			}); err != nil {
				return err
			}

			if err := flush(); err != nil {
				return err
			}

			if err := m.keepRefs(entity, src.refs(entity)); err != nil {
				return err
			}
		}

		return src.kv(func(key string, value []byte, expiresAt int64) error {
			report.KVKeys++
			return m.KVPut(key, value, expiresAt)
		})
	}()

	if writeErr != nil {
		m.Abandon()
		return nil, writeErr
	}

	if err := m.Close(); err != nil {
		return nil, err
	}

	return report, nil
}

func (e *Engine) keepRefs(name string, refs []RefPath) error {
	ent := e.entity(name, false)
	if ent == nil || len(refs) == 0 {
		return nil
	}

	ent.mu.Lock()
	defer ent.mu.Unlock()

	rec := ent.rec.Load()
	if rec == nil {
		return nil
	}

	next := rec.clone()
	changed := false
	for _, ref := range refs {
		if !next.hasRef(ref) {
			next.addRef(ref)
			changed = true
		}
	}

	if !changed {
		return nil
	}

	return e.writeRecord(ent, next, nil)
}

func verifyEntity(snap *Snapshot, entity string, src migrationSource) error {
	i := 0
	var it *scanner
	if snap != nil && snap.p != nil {
		lower := partPrefix(snap.id, partDocs)
		var err error
		if it, err = snap.p.iter(lower, prefixEnd(lower)); err != nil {
			return err
		}
		defer it.Close()
		it.First()
	}

	err := src.each(entity, func(doc migrationDoc) error {
		if it == nil || !it.Valid() {
			return fmt.Errorf("%s holds %d documents, more expected", entity, i)
		}

		_, id, value, ok := decodeDocValue(it.Value())
		if !ok || string(id) != doc.id || !bytes.Equal(value, doc.value) {
			return fmt.Errorf("%s document #%d (%s) differs after migration", entity, i, doc.id)
		}

		i++
		it.Next()

		return nil
	})
	if err != nil {
		return err
	}

	stored := 0
	if snap != nil {
		stored = snap.Len()
	}

	if (it != nil && it.Valid()) || stored != i {
		return fmt.Errorf("%s holds %d documents, %d expected", entity, stored, i)
	}

	return nil
}

func verifyMigration(dir string, shards int, src migrationSource) error {
	m, err := Open(migrationOptions(dir, shards))
	if err != nil {
		return err
	}
	defer m.Abandon()

	want := src.types()
	got := m.Types()
	if len(got) != len(want) {
		return fmt.Errorf("%d entity types migrated, %d expected", len(got), len(want))
	}

	for i := range want {
		if got[i] != want[i] {
			return fmt.Errorf("entity type #%d is %q, %q expected", i, got[i], want[i])
		}
	}

	snaps := m.SnapshotAll()
	defer func() {
		for _, s := range snaps {
			s.Release()
		}
	}()

	for _, entity := range src.entities() {
		if err := verifyEntity(snaps[entity], entity, src); err != nil {
			return err
		}
	}

	return src.kv(func(key string, value []byte, expiresAt int64) error {
		got, exp, ok := m.KVGet(key)
		if !ok || !bytes.Equal(got, value) || exp != expiresAt {
			return fmt.Errorf("key %q differs after migration", key)
		}
		return nil
	})
}

type legacySource struct {
	dir  string
	data *legacy.Data
}

func (s *legacySource) types() []string {
	return s.data.Types
}

func (s *legacySource) entities() []string {
	return s.data.Types
}

func (s *legacySource) refs(string) []RefPath {
	return nil
}

func (s *legacySource) each(entity string, fn func(doc migrationDoc) error) error {
	for _, doc := range s.data.Entities[entity] {
		value, err := json.Marshal(doc.Doc)
		if err != nil {
			return err
		}

		if err := fn(migrationDoc{id: doc.ID, value: value}); err != nil {
			return err
		}
	}

	return nil
}

func (s *legacySource) kv(fn func(key string, value []byte, expiresAt int64) error) error {
	keys := make([]string, 0, len(s.data.KV))
	for key := range s.data.KV {
		if !strings.HasPrefix(key, legacy.ApiKeyPrefix) {
			keys = append(keys, key)
		}
	}
	sort.Strings(keys)

	for _, key := range keys {
		if err := fn(key, s.data.KV[key], s.data.Expirations[key]); err != nil {
			return err
		}
	}

	return nil
}

func (s *legacySource) usedBytes() int64 {
	var used int64
	for _, name := range legacy.FilesIn(s.dir) {
		if info, err := os.Stat(filepath.Join(s.dir, name)); err == nil {
			used += info.Size()
		}
	}

	return used
}

type logSource struct {
	dir string
	f   *logengine.Folder
}

func (s *logSource) types() []string {
	return s.f.Types()
}

func (s *logSource) entities() []string {
	return s.f.Entities()
}

func (s *logSource) refs(entity string) []RefPath {
	var out []RefPath
	for _, r := range s.f.Refs(entity) {
		out = append(out, RefPath{Path: r.Path, Target: r.Target})
	}

	return out
}

func (s *logSource) each(entity string, fn func(doc migrationDoc) error) error {
	return s.f.Each(entity, func(doc logengine.Document) error {
		return fn(migrationDoc{id: doc.ID, value: doc.Value})
	})
}

func (s *logSource) kv(fn func(key string, value []byte, expiresAt int64) error) error {
	return s.f.KV(func(kv logengine.KV) error {
		return fn(kv.Key, kv.Value, kv.ExpiresAt)
	})
}

func (s *logSource) usedBytes() int64 {
	var used int64
	for _, name := range logengine.FilesIn(s.dir) {
		_ = filepath.Walk(filepath.Join(s.dir, name), func(_ string, info os.FileInfo, err error) error {
			if err == nil && !info.IsDir() {
				used += info.Size()
			}
			return nil
		})
	}

	return used
}

func (e *Engine) ExportLegacy(folder string) (*legacy.Data, error) {
	snaps := e.SnapshotAll()
	defer func() {
		for _, s := range snaps {
			s.Release()
		}
	}()

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
