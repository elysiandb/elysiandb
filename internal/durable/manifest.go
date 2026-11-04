package durable

import (
	"errors"
	"io"
	"io/fs"
	"path/filepath"
)

const (
	ManifestFile     = "MANIFEST"
	PreviousManifest = "MANIFEST.prev"
)

type Manifests struct {
	fs   FS
	dir  string
	last []byte
}

type StoredManifest struct {
	Name string
	Data []byte
}

func OpenManifests(f FS, dir string) *Manifests {
	return &Manifests{fs: Or(f), dir: dir}
}

func (m *Manifests) Stored() ([]StoredManifest, error) {
	var out []StoredManifest
	var firstErr error
	for _, name := range []string{ManifestFile, PreviousManifest} {
		data, err := ReadFile(m.fs, filepath.Join(m.dir, name))
		switch {
		case err == nil:
			out = append(out, StoredManifest{Name: name, Data: data})
		case errors.Is(err, fs.ErrNotExist):
		case firstErr == nil:
			firstErr = err
		}
	}

	if len(out) == 0 {
		return nil, firstErr
	}

	return out, nil
}

func (m *Manifests) Adopt(data []byte) {
	m.last = append([]byte(nil), data...)
}

func (m *Manifests) Commit(data []byte) error {
	if m.last != nil {
		if err := writeBytes(m.fs, filepath.Join(m.dir, PreviousManifest), m.last); err != nil {
			return err
		}
	}

	if err := writeBytes(m.fs, filepath.Join(m.dir, ManifestFile), data); err != nil {
		return err
	}

	m.Adopt(data)

	return nil
}

func writeBytes(f FS, path string, data []byte) error {
	return WriteFileAtomic(f, path, func(w io.Writer) error {
		_, err := w.Write(data)
		return err
	})
}
