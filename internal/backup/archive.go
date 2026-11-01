package backup

import (
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"time"
)

const (
	Format        = "elysiandb-backup"
	FormatVersion = 1

	ManifestPath = "manifest.json"
	KeyPath      = "security/users.key"
	KVPath       = "kv/keys.jsonl"

	KVEncodingBase64 = "base64"

	schemasFolder  = "schemas/"
	entitiesFolder = "entities/"

	fileNamePrefix    = "elysiandb-backup-"
	fileNameLayout    = "20060102T150405Z"
	FileNameExtension = ".tar.gz"
)

type Manifest struct {
	Format           string            `json:"format"`
	FormatVersion    int               `json:"format_version"`
	ElysianDBVersion string            `json:"elysiandb_version"`
	CreatedAt        string            `json:"created_at"`
	Engine           string            `json:"engine"`
	Entities         []EntityManifest  `json:"entities"`
	Schemas          []string          `json:"schemas"`
	KVKeys           int               `json:"kv_keys"`
	Files            map[string]string `json:"files"`
}

type EntityManifest struct {
	Name      string `json:"name"`
	Documents int    `json:"documents"`
}

type KVEntry struct {
	Key       string `json:"key"`
	Value     string `json:"value"`
	Encoding  string `json:"encoding,omitempty"`
	ExpiresAt int64  `json:"expires_at,omitempty"`
}

type ArchiveError struct {
	Reason string
}

func (e *ArchiveError) Error() string {
	return "invalid backup archive: " + e.Reason
}

type IncompleteRestoreError struct {
	Err error
}

func (e *IncompleteRestoreError) Error() string {
	return "the restore failed after the data was replaced, run it again: " + e.Err.Error()
}

func (e *IncompleteRestoreError) Unwrap() error {
	return e.Err
}

func invalidArchive(format string, args ...any) error {
	return &ArchiveError{Reason: fmt.Sprintf(format, args...)}
}

func FileName(t time.Time) string {
	return fileNamePrefix + t.UTC().Format(fileNameLayout) + FileNameExtension
}

func SchemaPath(entity string) string {
	return schemasFolder + escapeName(entity) + ".json"
}

func EntityPath(entity string) string {
	return entitiesFolder + escapeName(entity) + ".jsonl"
}

func (m *Manifest) paths() []string {
	paths := make([]string, 0, len(m.Schemas)+len(m.Entities)+2)
	paths = append(paths, KeyPath)

	for _, entity := range m.Schemas {
		paths = append(paths, SchemaPath(entity))
	}

	for _, entity := range m.Entities {
		paths = append(paths, EntityPath(entity.Name))
	}

	return append(paths, KVPath)
}

func escapeName(name string) string {
	var b strings.Builder
	for i := 0; i < len(name); i++ {
		c := name[i]
		if isSafeNameByte(c) {
			b.WriteByte(c)
			continue
		}

		fmt.Fprintf(&b, "%%%02X", c)
	}

	return b.String()
}

func isSafeNameByte(c byte) bool {
	return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-' || c == '.'
}

func newEncoder(w io.Writer) *json.Encoder {
	encoder := json.NewEncoder(w)
	encoder.SetEscapeHTML(false)

	return encoder
}
