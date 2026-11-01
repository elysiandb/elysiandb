package backup_test

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

type entry struct {
	name string
	data []byte
}

func setup(t *testing.T, mode string) string {
	t.Helper()

	folder := t.TempDir()
	cfg := &configuration.Config{}
	cfg.Store.Folder = folder
	cfg.Store.Shards = 4
	cfg.Store.CrashRecovery.Enabled = true
	cfg.Store.CrashRecovery.MaxLogMB = 10
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true
	cfg.Security.Authentication.Enabled = mode != ""
	cfg.Security.Authentication.Mode = mode
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatalf("cannot create the default admin: %v", err)
	}

	acl.InitACL()

	return folder
}

func create(t *testing.T) []byte {
	t.Helper()

	var buf bytes.Buffer
	if _, err := backup.Create(&buf, time.Date(2026, 11, 1, 9, 14, 0, 0, time.UTC)); err != nil {
		t.Fatalf("backup failed: %v", err)
	}

	return buf.Bytes()
}

func restore(t *testing.T, archive []byte) *backup.Report {
	t.Helper()

	report, err := backup.Restore(bytes.NewReader(archive))
	if err != nil {
		t.Fatalf("restore failed: %v", err)
	}

	return report
}

func entries(t *testing.T, archive []byte) []entry {
	t.Helper()

	gz, err := gzip.NewReader(bytes.NewReader(archive))
	if err != nil {
		t.Fatalf("not a gzip archive: %v", err)
	}

	var out []entry
	tr := tar.NewReader(gz)
	for {
		header, err := tr.Next()
		if err == io.EOF {
			return out
		}

		if err != nil {
			t.Fatalf("not a tar archive: %v", err)
		}

		data, _ := io.ReadAll(tr)
		out = append(out, entry{name: header.Name, data: data})
	}
}

func rebuild(t *testing.T, archive []byte, mutate func(name string, data []byte) []byte) []byte {
	t.Helper()

	list := entries(t, archive)
	var manifest map[string]any
	json.Unmarshal(list[0].data, &manifest)

	sums := map[string]any{}
	for i := range list[1:] {
		e := &list[i+1]
		e.data = mutate(e.name, e.data)
		sum := sha256.Sum256(e.data)
		sums[e.name] = hex.EncodeToString(sum[:])
	}

	manifest["files"] = sums
	list[0].data, _ = json.Marshal(manifest)

	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	for _, e := range list {
		tw.WriteHeader(&tar.Header{Name: e.name, Mode: 0o600, Size: int64(len(e.data)), Typeflag: tar.TypeReg})
		tw.Write(e.data)
	}

	tw.Close()
	gz.Close()

	return buf.Bytes()
}

func manifestOf(t *testing.T, archive []byte) backup.Manifest {
	t.Helper()

	var manifest backup.Manifest
	if err := json.Unmarshal(entries(t, archive)[0].data, &manifest); err != nil {
		t.Fatalf("invalid manifest: %v", err)
	}

	return manifest
}

func TestCreateWritesTheManifestFirstAndChecksumsEveryFile(t *testing.T) {
	setup(t, "user")

	api_storage.UpdateEntitySchema("book", map[string]any{"title": map[string]any{"type": "string", "required": true}})
	api_storage.WriteEntity("book", map[string]any{"id": "b1", "title": "Dune"})
	api_storage.AddEntityType("empty")
	storage.PutKeyValue("app:key", []byte("v"))

	archive := create(t)
	list := entries(t, archive)
	if list[0].name != backup.ManifestPath || list[1].name != backup.KeyPath || list[len(list)-1].name != backup.KVPath {
		t.Fatalf("unexpected archive layout %v", names(list))
	}

	manifest := manifestOf(t, archive)
	if manifest.Format != backup.Format || manifest.FormatVersion != backup.FormatVersion || manifest.Engine != "internal" || manifest.CreatedAt != "2026-11-01T09:14:00Z" || manifest.KVKeys != 1 {
		t.Fatalf("unexpected manifest %+v", manifest)
	}

	if len(manifest.Files) != len(list)-1 {
		t.Fatalf("expected a checksum for every file, got %v for %v", manifest.Files, names(list))
	}

	for _, e := range list[1:] {
		sum := sha256.Sum256(e.data)
		if manifest.Files[e.name] != hex.EncodeToString(sum[:]) {
			t.Fatalf("wrong checksum for %s", e.name)
		}
	}

	counts := map[string]int{}
	for _, e := range manifest.Entities {
		counts[e.Name] = e.Documents
	}

	if counts["book"] != 1 || counts["empty"] != 0 || counts[security.UserEntity] != 1 {
		t.Fatalf("unexpected document counts %v", counts)
	}

	if _, ok := counts[schema.SchemaEntity]; ok {
		t.Fatalf("schemas must not be listed as an entity: %v", counts)
	}

	key, _ := security.CreateKeyFileOrGetKey()
	if string(list[1].data) != key {
		t.Fatalf("expected the users key in the archive")
	}

	if !strings.Contains(strings.Join(names(list), ","), "schemas/book.json") {
		t.Fatalf("expected the book schema in the archive, got %v", names(list))
	}
}

func names(list []entry) []string {
	out := make([]string, 0, len(list))
	for _, e := range list {
		out = append(out, e.name)
	}

	return out
}

func TestFileNameIsUTCInBasicFormat(t *testing.T) {
	paris := time.FixedZone("CET", 3600)
	if got := backup.FileName(time.Date(2026, 11, 1, 10, 14, 0, 0, paris)); got != "elysiandb-backup-20261101T091400Z.tar.gz" {
		t.Fatalf("unexpected file name %s", got)
	}
}

func TestEntityNamesAreEscapedInPaths(t *testing.T) {
	setup(t, "")

	for _, entity := range []string{"a/b c", "../x", "Ω"} {
		api_storage.WriteEntity(entity, map[string]any{"id": "1", "v": entity})
	}

	archive := create(t)
	for _, entity := range []string{"a/b c", "../x", "Ω"} {
		for _, path := range []string{backup.EntityPath(entity), backup.SchemaPath(entity)} {
			folder, file, _ := strings.Cut(path, "/")
			if (folder != "entities" && folder != "schemas") || strings.ContainsAny(file, "/ ") || !strings.Contains(strings.Join(names(entries(t, archive)), ","), path) {
				t.Fatalf("unsafe or missing path %q in %v", path, names(entries(t, archive)))
			}
		}
	}

	setup(t, "")
	restore(t, archive)

	for _, entity := range []string{"a/b c", "../x", "Ω"} {
		if got := api_storage.ReadEntityById(entity, "1"); got == nil || got["v"] != entity {
			t.Fatalf("expected %q to be restored, got %v", entity, got)
		}
	}
}

func TestRestoreReplacesEverythingAndPersists(t *testing.T) {
	setup(t, "user")

	if err := security.CreateBasicUser(&security.BasicUser{Username: "alice", Password: "alice-pw", Role: security.RoleAdmin}); err != nil {
		t.Fatalf("cannot create alice: %v", err)
	}

	api_storage.WriteEntity("book", map[string]any{"id": "b2", "title": "Dune"})
	api_storage.WriteEntity("book", map[string]any{"id": "b1", "title": "Alien"})
	acl.InitACL()
	storage.PutKeyValue("app:plain", []byte("v1"))
	storage.PutKeyValue("app:binary", []byte{0xff, 0x00, 0xfe})
	expiresAt := time.Now().Unix() + 3600
	storage.PutKeyValueWithExpiration("app:ttl", []byte("soon"), expiresAt)
	archive := create(t)

	folder := setup(t, "user")
	api_storage.WriteEntity("stale", map[string]any{"id": "s1"})
	storage.PutKeyValue("target:only", []byte("x"))
	if _, err := security.CreateSession("admin", security.RoleAdmin, time.Hour); err != nil {
		t.Fatalf("cannot create a session: %v", err)
	}

	report := restore(t, archive)
	if report.Entities["book"] != 2 || report.KVKeys != 3 || report.ExpiredKVKeys != 0 {
		t.Fatalf("unexpected report %+v", report)
	}

	if _, err := os.Stat(filepath.Join(folder, security.SessionsFilename)); !os.IsNotExist(err) {
		t.Fatalf("expected the restore to end every session, got %v", err)
	}

	storage.LoadDB()
	storage.LoadJsonDB()

	if _, ok := security.AuthenticateUser("alice", "alice-pw"); !ok {
		t.Fatalf("expected alice to log in with her password after the restore")
	}

	if ids := listIDs("book"); !reflect.DeepEqual(ids, []string{"b2", "b1"}) {
		t.Fatalf("expected the list order of the backup, got %v", ids)
	}

	if api_storage.ReadEntityById("stale", "s1") != nil || api_storage.EntityTypeExists("stale") {
		t.Fatalf("data that is not in the archive must be gone")
	}

	if _, err := storage.GetByKey("target:only"); err == nil {
		t.Fatalf("KV keys that are not in the archive must be gone")
	}

	for key, value := range map[string][]byte{"app:plain": []byte("v1"), "app:binary": {0xff, 0x00, 0xfe}, "app:ttl": []byte("soon")} {
		if got, err := storage.GetByKey(key); err != nil || !bytes.Equal(got, value) {
			t.Fatalf("expected %s=%v after the restore, got %v (%v)", key, value, got, err)
		}
	}

	if got, ok := storage.GetKeyExpiration("app:ttl"); !ok || got != expiresAt {
		t.Fatalf("expected app:ttl to expire at %d, got %d (%v)", expiresAt, got, ok)
	}

	if a := acl.GetACLEntityForUsername("book", "alice"); a == nil || !a.Can(acl.PermissionRead) {
		t.Fatalf("expected the ACLs of the backup, got %v", a)
	}
}

func listIDs(entity string) []string {
	var ids []string
	for _, item := range api_storage.ListEntities(entity, 0, 0, "", true, nil, "", "") {
		ids = append(ids, item["id"].(string))
	}

	return ids
}

func TestRestoreSkipsExpiredKeys(t *testing.T) {
	setup(t, "")
	storage.PutKeyValueWithExpiration("app:old", []byte("x"), time.Now().Unix()+3600)
	storage.PutKeyValue("app:new", []byte("y"))
	archive := rebuild(t, create(t), func(name string, data []byte) []byte {
		if name != backup.KVPath {
			return data
		}

		var out []string
		for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
			var e backup.KVEntry
			json.Unmarshal([]byte(line), &e)
			if e.ExpiresAt > 0 {
				e.ExpiresAt = time.Now().Unix() - 10
			}

			b, _ := json.Marshal(e)
			out = append(out, string(b))
		}

		return []byte(strings.Join(out, "\n") + "\n")
	})

	setup(t, "")
	report := restore(t, archive)
	if report.KVKeys != 1 || report.ExpiredKVKeys != 1 {
		t.Fatalf("expected one restored and one expired key, got %+v", report)
	}

	if _, err := storage.GetByKey("app:old"); err == nil {
		t.Fatalf("an expired key must not be restored")
	}
}

func TestRestoreLeavesTheDataUntouchedOnAnInvalidArchive(t *testing.T) {
	setup(t, "")
	api_storage.WriteEntity("book", map[string]any{"id": "b1", "title": "Dune"})
	archive := create(t)

	setup(t, "")
	api_storage.WriteEntity("keep", map[string]any{"id": "k1"})
	keyBefore, _ := security.CreateKeyFileOrGetKey()

	invalid := rebuild(t, archive, func(name string, data []byte) []byte {
		if name == backup.EntityPath("book") {
			return []byte("{\"id\":\"b1\"}\n{\"id\":\"b1\"}\n")
		}

		return data
	})

	_, err := backup.Restore(bytes.NewReader(invalid))
	var archiveErr *backup.ArchiveError
	if !errors.As(err, &archiveErr) {
		t.Fatalf("expected an archive error, got %v", err)
	}

	if api_storage.ReadEntityById("keep", "k1") == nil || api_storage.ReadEntityById("book", "b1") != nil {
		t.Fatalf("a refused archive must leave the data untouched")
	}

	if keyAfter, _ := security.CreateKeyFileOrGetKey(); keyAfter != keyBefore {
		t.Fatalf("a refused archive must leave the users key untouched")
	}
}

func TestWriteFileNamesTheArchiveAfterItsDate(t *testing.T) {
	setup(t, "")
	folder := t.TempDir()
	now := time.Date(2026, 11, 1, 9, 14, 0, 0, time.UTC)

	path, manifest, err := backup.WriteFile(folder, now)
	if err != nil {
		t.Fatalf("backup failed: %v", err)
	}

	if path != filepath.Join(folder, "elysiandb-backup-20261101T091400Z.tar.gz") || manifest.CreatedAt != "2026-11-01T09:14:00Z" {
		t.Fatalf("unexpected backup %s %+v", path, manifest)
	}

	info, err := os.Stat(path)
	if err != nil || info.Mode().Perm() != 0o600 {
		t.Fatalf("expected a private archive, got %v (%v)", info, err)
	}

	if _, _, err := backup.WriteFile(folder, now); err == nil {
		t.Fatalf("an existing backup must not be overwritten")
	}

	files, _ := os.ReadDir(folder)
	if len(files) != 1 {
		t.Fatalf("expected no temporary file left, got %v", files)
	}
}
