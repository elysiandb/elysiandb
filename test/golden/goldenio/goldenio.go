package goldenio

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
)

const Baseline = "1a11e5e"

const ManifestName = "MANIFEST.json"

type ManifestFile struct {
	Path   string `json:"path"`
	Size   int64  `json:"size"`
	SHA256 string `json:"sha256"`
}

type Manifest struct {
	Part      string         `json:"part"`
	Baseline  string         `json:"baseline"`
	GoVersion string         `json:"go_version"`
	GOOS      string         `json:"goos"`
	GOARCH    string         `json:"goarch"`
	Files     []ManifestFile `json:"files"`
}

func PrepareOut(out, part string) error {
	abs, err := filepath.Abs(out)
	if err != nil {
		return err
	}
	if filepath.Base(abs) != part || filepath.Base(filepath.Dir(abs)) != "golden" || filepath.Base(filepath.Dir(filepath.Dir(abs))) != "tests" {
		return fmt.Errorf("output folder must be tests/golden/%s, got %s", part, out)
	}
	if err := os.RemoveAll(abs); err != nil {
		return err
	}
	return os.MkdirAll(abs, 0o755)
}

func WriteFile(out, rel string, data []byte) error {
	path := filepath.Join(out, filepath.FromSlash(rel))
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	return os.WriteFile(path, data, 0o644)
}

func MarshalIndent(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(true)
	enc.SetIndent("", "  ")
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func WriteJSON(out, rel string, v any) error {
	data, err := MarshalIndent(v)
	if err != nil {
		return err
	}
	return WriteFile(out, rel, data)
}

type Lines struct {
	buf bytes.Buffer
	err error
	n   int
}

func (l *Lines) Add(v any) {
	if l.err != nil {
		return
	}
	data, err := json.Marshal(v)
	if err != nil {
		l.err = err
		return
	}
	l.buf.Write(data)
	l.buf.WriteByte('\n')
	l.n++
}

func (l *Lines) Len() int {
	return l.n
}

func (l *Lines) Write(out, rel string) error {
	if l.err != nil {
		return l.err
	}
	return WriteFile(out, rel, l.buf.Bytes())
}

func WriteManifest(out, part string) error {
	m := Manifest{
		Part:      part,
		Baseline:  Baseline,
		GoVersion: runtime.Version(),
		GOOS:      runtime.GOOS,
		GOARCH:    runtime.GOARCH,
		Files:     []ManifestFile{},
	}
	err := filepath.WalkDir(out, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		rel, err := filepath.Rel(out, path)
		if err != nil {
			return err
		}
		rel = filepath.ToSlash(rel)
		if rel == ManifestName {
			return nil
		}
		if strings.HasSuffix(rel, ".txt") || strings.HasSuffix(rel, ".sh") {
			return fmt.Errorf("golden file %s has an extension ignored by .gitignore", rel)
		}
		for _, seg := range strings.Split(rel, "/") {
			if seg == "elysiandb" {
				return fmt.Errorf("golden path %s holds a segment ignored by .gitignore", rel)
			}
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		sum := sha256.Sum256(data)
		m.Files = append(m.Files, ManifestFile{Path: rel, Size: int64(len(data)), SHA256: hex.EncodeToString(sum[:])})
		return nil
	})
	if err != nil {
		return err
	}
	sort.Slice(m.Files, func(i, j int) bool { return m.Files[i].Path < m.Files[j].Path })
	return WriteJSON(out, ManifestName, m)
}

func Main(part string, run func(out string) error) {
	out := flag.String("out", "", "output folder tests/golden/"+part)
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "missing -out")
		os.Exit(2)
	}
	if err := PrepareOut(*out, part); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := run(*out); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := WriteManifest(*out, part); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
