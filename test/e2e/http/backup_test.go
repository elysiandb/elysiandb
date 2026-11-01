package e2e

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"mime/multipart"
	"reflect"
	"regexp"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
)

type archiveEntry struct {
	name string
	data []byte
}

func readArchive(t *testing.T, archive []byte) []archiveEntry {
	t.Helper()

	gz, err := gzip.NewReader(bytes.NewReader(archive))
	if err != nil {
		t.Fatalf("the backup is not a gzip file: %v", err)
	}

	var entries []archiveEntry
	tr := tar.NewReader(gz)
	for {
		header, err := tr.Next()
		if err == io.EOF {
			return entries
		}

		if err != nil {
			t.Fatalf("the backup is not a tar archive: %v", err)
		}

		data, _ := io.ReadAll(tr)
		entries = append(entries, archiveEntry{name: header.Name, data: data})
	}
}

func writeArchive(t *testing.T, entries []archiveEntry) []byte {
	t.Helper()

	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	for _, e := range entries {
		if err := tw.WriteHeader(&tar.Header{Name: e.name, Mode: 0o600, Size: int64(len(e.data)), Typeflag: tar.TypeReg}); err != nil {
			t.Fatalf("cannot write %s: %v", e.name, err)
		}

		tw.Write(e.data)
	}

	tw.Close()
	gz.Close()

	return buf.Bytes()
}

func archiveContent(t *testing.T, archive []byte) map[string]string {
	t.Helper()

	createdAt := regexp.MustCompile(`"created_at": "[^"]*"`)
	content := map[string]string{}
	for _, e := range readArchive(t, archive) {
		content[e.name] = createdAt.ReplaceAllString(string(e.data), "")
	}

	return content
}

func mutateArchive(t *testing.T, archive []byte, mutate func(name string, data []byte) []byte) []byte {
	t.Helper()

	entries := readArchive(t, archive)
	for i := range entries {
		entries[i].data = mutate(entries[i].name, entries[i].data)
	}

	return writeArchive(t, entries)
}

func rewriteArchive(t *testing.T, archive []byte, mutate func(name string, data []byte) []byte) []byte {
	t.Helper()

	entries := readArchive(t, archive)
	sums := map[string]any{}
	for i := range entries {
		entries[i].data = mutate(entries[i].name, entries[i].data)
		if entries[i].name != "manifest.json" {
			sum := sha256.Sum256(entries[i].data)
			sums[entries[i].name] = hex.EncodeToString(sum[:])
		}
	}

	var manifest map[string]any
	if err := json.Unmarshal(entries[0].data, &manifest); err != nil {
		t.Fatalf("invalid manifest: %v", err)
	}

	manifest["files"] = sums
	entries[0].data, _ = json.Marshal(manifest)

	return writeArchive(t, entries)
}

func replaceEntry(entry, content string) func(name string, data []byte) []byte {
	return func(name string, data []byte) []byte {
		if name != entry {
			return data
		}

		return []byte(content)
	}
}

func replaceIn(entry, old, new string) func(name string, data []byte) []byte {
	return func(name string, data []byte) []byte {
		if name != entry {
			return data
		}

		return bytes.Replace(data, []byte(old), []byte(new), 1)
	}
}

func populateBackupSource(t *testing.T) (*apiServer, map[string]string) {
	t.Helper()

	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/admin/password", `{"password":"admin-secret"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"alice","password":"alice-pw","role":"admin"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"carol","password":"carol-pw","role":"user"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book/create", `{"fields":{"title":{"type":"string","required":true},"year":{"type":"number"}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/empty/create", `{"fields":{"x":{"type":"string","required":true}}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"id":"b2","title":"Dune","year":1965}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"id":"b1","title":"Alien"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/movie", `{"id":"m1","title":"Heat","tags":["a","b"],"meta":{"rating":4.5}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/book", `{"name":"tag","event":"post_read","language":"javascript","priority":5,"enabled":true,"script":"function postRead(ctx) { ctx.entity.tagged = true }"}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/acl/carol/book", `{"permissions":{"read":true,"create":false,"update":false,"delete":false,"owning_read":true,"owning_write":true,"owning_update":true,"owning_delete":true}}`)
	s.mustDo(fasthttp.StatusNoContent, "PUT", "/kv/app:marker", "v1")
	s.mustDo(fasthttp.StatusNoContent, "PUT", "/kv/app:session?ttl=3600", "short-lived")
	s.mustDo(fasthttp.StatusNoContent, "PUT", "/kv/app:binary", "\xff\xfe\x00raw")

	state := map[string]string{
		"book":        string(s.mustDo(fasthttp.StatusOK, "GET", "/api/book", "").body),
		"book schema": string(s.mustDo(fasthttp.StatusOK, "GET", "/api/book/schema", "").body),
		"types":       string(s.mustDo(fasthttp.StatusOK, "GET", "/api/entity/types", "").body),
		"hooks":       string(s.mustDo(fasthttp.StatusOK, "GET", "/api/hook/book", "").body),
		"acl alice":   string(s.mustDo(fasthttp.StatusOK, "GET", "/api/acl/alice", "").body),
		"acl carol":   string(s.mustDo(fasthttp.StatusOK, "GET", "/api/acl/carol", "").body),
		"users":       string(s.mustDo(fasthttp.StatusOK, "GET", "/api/security/user", "").body),
	}

	return s, state
}

func TestBackupAndRestoreRoundTrip(t *testing.T) {
	source, state := populateBackupSource(t)
	sessionExpiration, _ := storage.GetKeyExpiration("app:session")

	resp := source.mustDo(fasthttp.StatusOK, "GET", "/api/backup", "")
	if got := resp.header("Content-Type"); got != "application/gzip" {
		t.Fatalf("expected application/gzip, got %q", got)
	}

	if got := resp.header("Content-Disposition"); !regexp.MustCompile(`^attachment; filename="elysiandb-backup-\d{8}T\d{6}Z\.tar\.gz"$`).MatchString(got) {
		t.Fatalf("unexpected Content-Disposition %q", got)
	}

	archive := resp.body

	target := startAdminSession(t, nil)
	target.mustDo(fasthttp.StatusOK, "POST", "/api/stale", `{"id":"s1"}`)
	target.mustDo(fasthttp.StatusNoContent, "PUT", "/kv/target:only", "x")

	report := target.mustDo(fasthttp.StatusOK, "POST", "/api/restore", string(archive), "Content-Type", "application/gzip").object(t)
	entities, _ := report["entities"].(map[string]any)
	if entities["book"] != float64(2) || entities["empty"] != float64(0) || entities["movie"] != float64(1) || report["kv_keys"] != float64(3) {
		t.Fatalf("unexpected restore report %v", report)
	}

	target.mustDo(fasthttp.StatusUnauthorized, "GET", "/api/book", "")

	anonymous := target.anonymous()
	for _, login := range []struct {
		username, password string
		status             int
	}{
		{"alice", "alice-pw", fasthttp.StatusOK},
		{"admin", "admin-secret", fasthttp.StatusOK},
		{"admin", "admin", fasthttp.StatusUnauthorized},
	} {
		body, _ := json.Marshal(map[string]string{"username": login.username, "password": login.password})
		anonymous.mustDo(login.status, "POST", "/api/security/login", string(body))
	}

	restored := target.anonymous()
	restored.login("admin", "admin-secret")

	for name, path := range map[string]string{
		"book":        "/api/book",
		"book schema": "/api/book/schema",
		"types":       "/api/entity/types",
		"hooks":       "/api/hook/book",
		"acl alice":   "/api/acl/alice",
		"acl carol":   "/api/acl/carol",
		"users":       "/api/security/user",
	} {
		if got := string(restored.mustDo(fasthttp.StatusOK, "GET", path, "").body); got != state[name] {
			t.Fatalf("%s changed after the restore:\nbefore %s\nafter  %s", name, state[name], got)
		}
	}

	if !strings.Contains(state["book"], `"tagged":true`) {
		t.Fatalf("expected the hook to run on the source, got %s", state["book"])
	}

	if names := restored.mustDo(fasthttp.StatusOK, "GET", "/api/entity/types/name", "").object(t)["entities"]; !reflect.DeepEqual(names, []any{"book", "empty", "movie"}) {
		t.Fatalf("expected the entity types of the backup only, got %v", names)
	}

	restored.mustDo(fasthttp.StatusOK, "GET", "/api/empty/schema", "")
	restored.mustDo(fasthttp.StatusBadRequest, "POST", "/api/book", `{"id":"b3"}`)
	restored.mustDo(fasthttp.StatusBadRequest, "POST", "/api/empty", `{"id":"e1"}`)
	restored.mustDo(fasthttp.StatusNotFound, "GET", "/api/stale/s1", "")
	restored.mustDo(fasthttp.StatusNotFound, "GET", "/kv/target:only", "")

	for key, value := range map[string]string{"app:marker": "v1", "app:session": "short-lived", "app:binary": "\xff\xfe\x00raw"} {
		got, err := storage.GetByKey(key)
		if err != nil || string(got) != value {
			t.Fatalf("expected %s=%q after the restore, got %q (%v)", key, value, got, err)
		}
	}

	if got, ok := storage.GetKeyExpiration("app:session"); !ok || got != sessionExpiration {
		t.Fatalf("expected app:session to keep its expiration %d, got %d (%v)", sessionExpiration, got, ok)
	}

	if _, ok := storage.GetKeyExpiration("app:marker"); ok {
		t.Fatalf("app:marker must not get an expiration")
	}

	if got := archiveContent(t, restored.mustDo(fasthttp.StatusOK, "GET", "/api/backup", "").body); !reflect.DeepEqual(got, archiveContent(t, archive)) {
		t.Fatalf("a second backup must hold the same content as the first")
	}

	restored.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"bob","password":"bob-pw","role":"admin"}`)
	if acl := restored.mustDo(fasthttp.StatusOK, "GET", "/api/acl/bob/book", "").object(t); acl["username"] != "bob" {
		t.Fatalf("expected a new user to get ACLs after the restore, got %v", acl)
	}

	restored.mustDo(fasthttp.StatusOK, "POST", "/api/film", `{"id":"f1","title":"Alien"}`)
	restored.mustDo(fasthttp.StatusOK, "GET", "/api/film/f1", "")
	if acl := restored.mustDo(fasthttp.StatusOK, "GET", "/api/acl/alice/film", "").object(t); acl["entity"] != "film" {
		t.Fatalf("expected a new entity type to get ACLs after the restore, got %v", acl)
	}
}

func TestRestoreAcceptsAnyFileName(t *testing.T) {
	source, _ := populateBackupSource(t)
	archive := source.mustDo(fasthttp.StatusOK, "GET", "/api/backup", "").body

	target := startAdminSession(t, nil)

	var body bytes.Buffer
	form := multipart.NewWriter(&body)
	part, _ := form.CreateFormFile("anything", "my-copy.bin")
	part.Write(archive)
	form.Close()

	target.mustDo(fasthttp.StatusOK, "POST", "/api/restore", body.String(), "Content-Type", form.FormDataContentType())

	restored := target.anonymous()
	restored.login("alice", "alice-pw")
	restored.mustDo(fasthttp.StatusOK, "GET", "/api/book/b1", "")
}

func TestRestoreRefusesInvalidArchivesAndKeepsTheData(t *testing.T) {
	source, _ := populateBackupSource(t)
	archive := source.mustDo(fasthttp.StatusOK, "GET", "/api/backup", "").body

	target := startAdminSession(t, nil)
	target.mustDo(fasthttp.StatusOK, "POST", "/api/keep", `{"id":"k1","v":1}`)
	target.mustDo(fasthttp.StatusNoContent, "PUT", "/kv/keep:me", "x")

	entries := readArchive(t, archive)
	bookLines := strings.SplitAfter(string(readEntry(t, entries, "entities/book.jsonl")), "\n")

	for name, tc := range map[string]struct {
		body    []byte
		message string
	}{
		"newer format":        {mutateArchive(t, archive, replaceIn("manifest.json", `"format_version": 1`, `"format_version": 2`)), "format version 2 is newer"},
		"unknown format":      {mutateArchive(t, archive, replaceIn("manifest.json", `"elysiandb-backup"`, `"other"`)), `unknown format "other"`},
		"bad checksum":        {mutateArchive(t, archive, replaceIn("entities/book.jsonl", "Dune", "Dunx")), "checksum mismatch for entities/book.jsonl"},
		"missing file":        {writeArchive(t, entries[:len(entries)-1]), "kv/keys.jsonl is missing"},
		"extra file":          {writeArchive(t, append(append([]archiveEntry{}, entries...), archiveEntry{name: "extra.txt", data: []byte("x")})), "unexpected file extra.txt"},
		"wrong order":         {writeArchive(t, append([]archiveEntry{entries[0], entries[2], entries[1]}, entries[3:]...)), "expected security/users.key"},
		"no checksums":        {mutateArchive(t, archive, dropChecksums), "no checksum for security/users.key"},
		"not a gzip":          {[]byte("not an archive"), "not a gzip file"},
		"gzip without tar":    {gzipBytes(t, []byte("plain text")), "invalid backup archive"},
		"truncated":           {archive[:len(archive)/2], "invalid backup archive"},
		"empty body":          {nil, "must be a backup archive"},
		"count mismatch":      {rewriteArchive(t, archive, replaceIn("entities/book.jsonl", bookLines[0], "")), "entities/book.jsonl holds 1 entries, the manifest announces 2"},
		"duplicate id":        {rewriteArchive(t, archive, replaceIn("entities/book.jsonl", bookLines[0], bookLines[0]+bookLines[0])), `duplicate id "b2"`},
		"document without id": {rewriteArchive(t, archive, replaceIn("entities/book.jsonl", `"id":"b2"`, `"id":""`)), "a document must be an object with a string id"},
		"invalid document":    {rewriteArchive(t, archive, replaceIn("entities/book.jsonl", `{`, `[`)), "entities/book.jsonl, document 1"},
		"foreign schema":      {rewriteArchive(t, archive, replaceIn("schemas/book.json", `"id":"book"`, `"id":"film"`)), `describes "film" instead of "book"`},
		"empty key":           {rewriteArchive(t, archive, replaceEntry("security/users.key", "\n")), "does not hold a valid key"},
		"internal KV key":     {rewriteArchive(t, archive, replaceIn("kv/keys.jsonl", `"app:marker"`, `"api:entity:types:list"`)), "invalid key"},
		"unknown KV field":    {rewriteArchive(t, archive, replaceIn("kv/keys.jsonl", `{"key"`, `{"extra":1,"key"`)), "unknown field"},
		"unknown KV encoding": {rewriteArchive(t, archive, replaceIn("kv/keys.jsonl", `"encoding":"base64"`, `"encoding":"hex"`)), `unknown encoding "hex"`},
	} {
		t.Run(name, func(t *testing.T) {
			resp := target.mustDo(fasthttp.StatusBadRequest, "POST", "/api/restore", string(tc.body))
			if message, _ := resp.object(t)["error"].(string); !strings.Contains(message, tc.message) {
				t.Fatalf("expected an error containing %q, got %s", tc.message, resp.body)
			}

			if got := target.mustDo(fasthttp.StatusOK, "GET", "/api/keep/k1", "").object(t)["v"]; got != float64(1) {
				t.Fatalf("a refused restore must leave the data untouched, got %v", got)
			}

			target.mustDo(fasthttp.StatusOK, "GET", "/kv/keep:me", "")
			target.mustDo(fasthttp.StatusNotFound, "GET", "/api/book/b1", "")
		})
	}
}

func gzipBytes(t *testing.T, data []byte) []byte {
	t.Helper()

	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	gz.Write(data)
	gz.Close()

	return buf.Bytes()
}

func readEntry(t *testing.T, entries []archiveEntry, name string) []byte {
	t.Helper()

	for _, e := range entries {
		if e.name == name {
			return e.data
		}
	}

	t.Fatalf("%s is not in the archive", name)

	return nil
}

func dropChecksums(name string, data []byte) []byte {
	if name != "manifest.json" {
		return data
	}

	var manifest map[string]any
	json.Unmarshal(data, &manifest)
	manifest["files"] = map[string]any{}
	out, _ := json.Marshal(manifest)

	return out
}

func TestBackupAndRestoreRequireAnAdmin(t *testing.T) {
	s := startAdminSession(t, nil)
	archive := s.mustDo(fasthttp.StatusOK, "GET", "/api/backup", "").body

	anonymous := s.anonymous()
	anonymous.mustDo(fasthttp.StatusUnauthorized, "GET", "/api/backup", "")
	anonymous.mustDo(fasthttp.StatusUnauthorized, "POST", "/api/restore", string(archive))
}
