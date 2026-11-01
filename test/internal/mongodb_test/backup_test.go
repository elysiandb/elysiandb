package mongodb_test

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"fmt"
	"io"
	"os"
	"reflect"
	"regexp"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
)

func backupConfig(t *testing.T, engineName string) *configuration.Config {
	t.Helper()

	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Engine.Name = engineName
	cfg.Engine.URI = os.Getenv("ELYSIAN_TEST_MONGODB_URI")
	cfg.Api.Schema.Enabled = true
	cfg.Api.Schema.Strict = true
	cfg.Api.Hooks.Enabled = true
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"

	return cfg
}

func startBackupInstance(t *testing.T, engineName string) {
	t.Helper()

	uri := os.Getenv("ELYSIAN_TEST_MONGODB_URI")
	if uri == "" {
		t.Skip("ELYSIAN_TEST_MONGODB_URI is not set")
	}

	globals.SetConfig(backupConfig(t, engineName))
	storage.LoadDB()
	storage.LoadJsonDB()

	if engineName == engine.EngineMongoDB {
		boot.InitMongoDBConnection()

		client := globals.MongoClient
		database := client.Database(fmt.Sprintf("elysiandb_backup_test_%d", time.Now().UnixNano()))
		globals.MongoDB = database

		t.Cleanup(func() {
			ctx := context.Background()
			_ = database.Drop(ctx)
			_ = client.Disconnect(ctx)
		})
	}

	if err := security.InitAdminUserIfNotExists(); err != nil {
		t.Fatalf("cannot create the default admin: %v", err)
	}

	hook.InitHooks()
	acl.InitACL()
}

func populateBackupInstance(t *testing.T) {
	t.Helper()

	if err := security.CreateBasicUser(&security.BasicUser{Username: "alice", Password: "alice-pw", Role: security.RoleAdmin}); err != nil {
		t.Fatalf("cannot create alice: %v", err)
	}

	engine.CreateEntityType("book")
	engine.UpdateEntitySchema("book", map[string]any{"title": map[string]any{"type": "string", "required": true}})
	engine.CreateEntityType("empty")
	engine.UpdateEntitySchema("empty", map[string]any{"x": map[string]any{"type": "string", "required": true}})

	for _, doc := range []map[string]any{{"id": "b2", "title": "Dune"}, {"id": "b1", "title": "Alien"}} {
		if errs := engine.WriteEntity("book", doc); len(errs) > 0 {
			t.Fatalf("cannot write %v: %v", doc, errs)
		}
	}

	if err := hook.CreateHook(&hook.Hook{Name: "tag", Entity: "book", Event: hook.HookEventPostRead, Language: "javascript", Priority: 5, Enabled: true}); err != nil {
		t.Fatalf("cannot create the hook: %v", err)
	}

	acl.InitACL()
	storage.PutKeyValue("app:marker", []byte("v1"))
	storage.PutKeyValueWithTTL("app:session", []byte("short"), 3600)
}

func createBackup(t *testing.T) []byte {
	t.Helper()

	var buf bytes.Buffer
	if _, err := backup.Create(&buf, time.Now()); err != nil {
		t.Fatalf("backup failed: %v", err)
	}

	return buf.Bytes()
}

func backupContent(t *testing.T, archive []byte) map[string]string {
	t.Helper()

	gz, err := gzip.NewReader(bytes.NewReader(archive))
	if err != nil {
		t.Fatalf("not a gzip archive: %v", err)
	}

	createdAt := regexp.MustCompile(`"(created_at|engine)": "[^"]*"`)
	content := map[string]string{}
	tr := tar.NewReader(gz)
	for {
		header, err := tr.Next()
		if err == io.EOF {
			return content
		}

		if err != nil {
			t.Fatalf("not a tar archive: %v", err)
		}

		data, _ := io.ReadAll(tr)
		content[header.Name] = createdAt.ReplaceAllString(string(data), "")
	}
}

func checkRestoredInstance(t *testing.T) {
	t.Helper()

	if _, ok := security.AuthenticateUser("alice", "alice-pw"); !ok {
		t.Fatalf("expected alice to log in with her password")
	}

	if got := engine.ReadEntityById("book", "b1"); got == nil || got["title"] != "Alien" {
		t.Fatalf("expected b1 to be restored, got %v", got)
	}

	if got := engine.ReadEntityById("stale", "s1"); got != nil || engine.EntityTypeExists("stale") {
		t.Fatalf("data that is not in the archive must be gone, got %v", got)
	}

	if !schema.IsManualSchema("empty", engine.GetEntitySchema("empty")) || !engine.EntityTypeExists("empty") {
		t.Fatalf("expected the empty entity type with its manual schema")
	}

	if errs := engine.WriteEntity("book", map[string]any{"id": "b3"}); len(errs) == 0 {
		t.Fatalf("expected the manual schema to be enforced")
	}

	if a := acl.GetACLEntityForUsername("book", "alice"); a == nil || !a.Can(acl.PermissionRead) {
		t.Fatalf("expected the ACL of alice, got %v", a)
	}

	if hooks, _ := hook.GetHooksForEntity("book"); len(hooks) != 1 || !hooks[0].Enabled || hooks[0].Priority != 5 {
		t.Fatalf("expected the hook to be restored, got %v", hooks)
	}

	if got, err := storage.GetByKey("app:marker"); err != nil || string(got) != "v1" {
		t.Fatalf("expected app:marker, got %q (%v)", got, err)
	}

	if _, ok := storage.GetKeyExpiration("app:session"); !ok {
		t.Fatalf("expected app:session to keep its expiration")
	}
}

func TestMongoDBBackupAndRestoreRoundTrip(t *testing.T) {
	startBackupInstance(t, engine.EngineMongoDB)
	populateBackupInstance(t)
	archive := createBackup(t)

	startBackupInstance(t, engine.EngineMongoDB)
	engine.WriteEntity("stale", map[string]any{"id": "s1"})

	report, err := backup.Restore(bytes.NewReader(archive))
	if err != nil {
		t.Fatalf("restore failed: %v", err)
	}

	if report.Entities["book"] != 2 || report.Entities["empty"] != 0 || report.KVKeys != 2 {
		t.Fatalf("unexpected report %+v", report)
	}

	checkRestoredInstance(t)

	if !reflect.DeepEqual(backupContent(t, createBackup(t)), backupContent(t, archive)) {
		t.Fatalf("a second backup must hold the same content as the first")
	}
}

func TestBackupMovesFromTheInternalEngineToMongoDB(t *testing.T) {
	startBackupInstance(t, engine.EngineInternal)
	populateBackupInstance(t)
	archive := createBackup(t)

	startBackupInstance(t, engine.EngineMongoDB)
	engine.WriteEntity("stale", map[string]any{"id": "s1"})

	if _, err := backup.Restore(bytes.NewReader(archive)); err != nil {
		t.Fatalf("restore failed: %v", err)
	}

	checkRestoredInstance(t)

	if !reflect.DeepEqual(backupContent(t, createBackup(t)), backupContent(t, archive)) {
		t.Fatalf("the archive must not depend on the engine")
	}
}
