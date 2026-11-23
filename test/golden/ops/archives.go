package main

import (
	"archive/tar"
	"bytes"
	"compress/flate"
	"compress/gzip"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"syscall"
	"time"
	"unicode/utf8"

	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/test/golden/goldenio"
	"golang.org/x/crypto/bcrypt"
)

const (
	archAdminPassword = "golden-admin"
	archLiveExpiry    = 4102444800
	archOldExpiry     = 1000000000
)

var archTime = time.Date(2026, 11, 23, 10, 20, 30, 0, time.UTC)

type archDetReader struct {
	n uint64
}

func (r *archDetReader) Read(p []byte) (int, error) {
	for i := 0; i < len(p); i += 32 {
		var seed [8]byte
		binary.LittleEndian.PutUint64(seed[:], r.n)
		r.n++
		sum := sha256.Sum256(append([]byte("archives"), seed[:]...))
		copy(p[i:], sum[:])
	}
	return len(p), nil
}

func archSHA(b []byte) string {
	s := sha256.Sum256(b)
	return hex.EncodeToString(s[:])
}

func archKey() []byte {
	var b bytes.Buffer
	seed := sha256.Sum256([]byte("elysiandb golden users.key"))
	for b.Len() < 1024 {
		b.WriteString(hex.EncodeToString(seed[:]))
		seed = sha256.Sum256(seed[:])
	}
	return b.Bytes()[:1024]
}

type archEntry struct {
	name string
	data []byte
	flag byte
}

type archNamed struct {
	name string
	data []byte
}

type archEntity struct {
	name string
	data []byte
	docs int
}

type archParts struct {
	key      []byte
	schemas  []archNamed
	entities []archEntity
	kv       []byte
	kvKeys   int
	extra    []archEntry
	raw      []byte
	edit     func(m *backup.Manifest)
	rawEdit  func([]byte) []byte
	order    func([]archEntry) []archEntry
	tarEdit  func([]byte) []byte
	gzEdit   func([]byte) []byte
	level    int
}

func archTar(entries []archEntry) []byte {
	var buf bytes.Buffer
	tw := tar.NewWriter(&buf)
	for _, e := range entries {
		flag := e.flag
		if flag == 0 {
			flag = tar.TypeReg
		}
		size := int64(len(e.data))
		if flag == tar.TypeDir {
			size = 0
		}
		if err := tw.WriteHeader(&tar.Header{Name: e.name, Mode: 0o600, Size: size, ModTime: archTime, Typeflag: flag}); err != nil {
			panic(err)
		}
		if size > 0 {
			if _, err := tw.Write(e.data); err != nil {
				panic(err)
			}
		}
	}
	if err := tw.Close(); err != nil {
		panic(err)
	}
	return buf.Bytes()
}

func archGzip(data []byte, level int) []byte {
	var buf bytes.Buffer
	gz, err := gzip.NewWriterLevel(&buf, level)
	if err != nil {
		panic(err)
	}
	gz.Write(data)
	gz.Close()
	return buf.Bytes()
}

func (p archParts) manifest() *backup.Manifest {
	m := &backup.Manifest{Format: backup.Format, FormatVersion: backup.FormatVersion, ElysianDBVersion: "0.3.0", CreatedAt: "2026-11-23T10:20:30Z", Engine: "internal", Entities: []backup.EntityManifest{}, Schemas: []string{}, KVKeys: p.kvKeys, Files: map[string]string{}}
	m.Files[backup.KeyPath] = archSHA(p.key)
	for _, s := range p.schemas {
		m.Schemas = append(m.Schemas, s.name)
		m.Files[backup.SchemaPath(s.name)] = archSHA(s.data)
	}
	for _, e := range p.entities {
		m.Entities = append(m.Entities, backup.EntityManifest{Name: e.name, Documents: e.docs})
		m.Files[backup.EntityPath(e.name)] = archSHA(e.data)
	}
	m.Files[backup.KVPath] = archSHA(p.kv)
	return m
}

func (p archParts) tar() []byte {
	m := p.manifest()
	if p.edit != nil {
		p.edit(m)
	}
	manifest := p.raw
	if manifest == nil {
		b, err := json.MarshalIndent(m, "", "  ")
		if err != nil {
			panic(err)
		}
		manifest = append(b, '\n')
	}
	if p.rawEdit != nil {
		manifest = p.rawEdit(manifest)
	}
	entries := []archEntry{{name: backup.ManifestPath, data: manifest}, {name: backup.KeyPath, data: p.key}}
	for _, s := range p.schemas {
		entries = append(entries, archEntry{name: backup.SchemaPath(s.name), data: s.data})
	}
	for _, e := range p.entities {
		entries = append(entries, archEntry{name: backup.EntityPath(e.name), data: e.data})
	}
	entries = append(entries, archEntry{name: backup.KVPath, data: p.kv})
	entries = append(entries, p.extra...)
	if p.order != nil {
		entries = p.order(entries)
	}
	t := archTar(entries)
	if p.tarEdit != nil {
		t = p.tarEdit(t)
	}
	return t
}

func (p archParts) build() []byte {
	level := p.level
	if level == 0 {
		level = gzip.BestSpeed
	}
	g := archGzip(p.tar(), level)
	if p.gzEdit != nil {
		g = p.gzEdit(g)
	}
	return g
}

func archBase() archParts {
	return archParts{
		key:      archKey(),
		schemas:  []archNamed{{name: "books", data: []byte(`{"fields":{"title":{"name":"title","required":false,"type":"string"}},"id":"books"}` + "\n")}},
		entities: []archEntity{{name: "books", data: []byte(`{"id":"b1","title":"One"}` + "\n" + `{"id":"b2","title":"Two"}` + "\n"), docs: 2}},
		kv:       []byte(`{"key":"k1","value":"v1"}` + "\n" + `{"key":"k2","value":"//4=","encoding":"base64"}` + "\n"),
		kvKeys:   2,
	}
}

func archSeed() (archParts, error) {
	key := archKey()
	saved := rand.Reader
	rand.Reader = &archDetReader{}
	sum := sha256.Sum256([]byte(archAdminPassword + string(key)))
	hash, err := bcrypt.GenerateFromPassword(sum[:], bcrypt.DefaultCost)
	rand.Reader = saved
	if err != nil {
		return archParts{}, err
	}
	admin := `{"id":"admin","password":"` + string(hash) + `","role":"admin","username":"admin"}` + "\n"
	schema := `{"_manual":true,"fields":{"id":{"name":"id","required":true,"type":"string"},"password":{"name":"password","required":true,"type":"string"},"role":{"name":"role","required":true,"type":"string"},"username":{"name":"username","required":true,"type":"string"}},"id":"_elysiandb_core_user"}` + "\n"
	kv := fmt.Sprintf(`{"key":"live","value":"until 2100","expires_at":%d}`+"\n"+`{"key":"old","value":"expired","expires_at":%d}`+"\n"+`{"key":"seed","value":"v"}`+"\n", archLiveExpiry, archOldExpiry)
	return archParts{
		key:      key,
		schemas:  []archNamed{{name: "_elysiandb_core_user", data: []byte(schema)}},
		entities: []archEntity{{name: "_elysiandb_core_user", data: []byte(admin), docs: 1}},
		kv:       []byte(kv),
		kvKeys:   3,
	}, nil
}

func archMarker(class string) string {
	return "\u27e8go:" + class + "\u27e9"
}

func archReplay(data []byte) (map[string]string, map[string]any) {
	texts := map[string]string{}
	values := map[string]any{}
	add := func(err error, class string) {
		if err == nil {
			return
		}
		if _, ok := texts[err.Error()]; !ok {
			texts[err.Error()] = class
		}
		var ce flate.CorruptInputError
		if errors.As(err, &ce) {
			values["flate_offset"] = int64(ce)
		}
	}
	gz, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		add(err, "gzip")
		return texts, values
	}
	_, gzErr := io.ReadAll(gz)
	add(gzErr, "gzip")
	streamClass := "tar"
	if gzErr != nil {
		streamClass = "gzip"
	}
	gz, _ = gzip.NewReader(bytes.NewReader(data))
	tr := tar.NewReader(gz)
	for {
		h, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			add(err, streamClass)
			break
		}
		content, err := io.ReadAll(tr)
		if err != nil {
			add(err, streamClass)
			break
		}
		archReplayJSON(h.Name, content, add)
	}
	return texts, values
}

func archReplayJSON(name string, content []byte, add func(error, string)) {
	switch {
	case name == backup.ManifestPath:
		var m backup.Manifest
		add(json.Unmarshal(content, &m), "json")
	case strings.HasPrefix(name, "entities/"):
		dec := json.NewDecoder(bytes.NewReader(content))
		for {
			var raw json.RawMessage
			if err := dec.Decode(&raw); err != nil {
				if err != io.EOF {
					add(err, "json")
				}
				return
			}
			var doc map[string]any
			add(json.Unmarshal(raw, &doc), "json")
		}
	case name == backup.KVPath:
		dec := json.NewDecoder(bytes.NewReader(content))
		dec.DisallowUnknownFields()
		for {
			var e backup.KVEntry
			if err := dec.Decode(&e); err != nil {
				if err != io.EOF {
					add(err, "json")
				}
				return
			}
		}
	}
}

func archMask(s string, texts map[string]string) string {
	keys := make([]string, 0, len(texts))
	for k := range texts {
		keys = append(keys, k)
	}
	sort.Slice(keys, func(i, j int) bool {
		if len(keys[i]) != len(keys[j]) {
			return len(keys[i]) > len(keys[j])
		}
		return keys[i] < keys[j]
	})
	for _, k := range keys {
		if k != "" {
			s = strings.ReplaceAll(s, k, archMarker(texts[k]))
		}
	}
	return s
}

func archMaskedReason(data []byte) (string, map[string]any, bool) {
	_, err := backup.Verify(bytes.NewReader(data))
	if err == nil {
		return "", nil, false
	}
	texts, values := archReplay(data)
	masked := archMask(err.Error(), texts)
	used := map[string]any{}
	if strings.Contains(masked, archMarker("gzip")) {
		for k, v := range values {
			used[k] = v
		}
	}
	return masked, used, true
}

type archCase struct {
	slug   string
	data   []byte
	rule3  func(string) string
	d20    string
	accept bool
}

func archRule3(goText string, value any) func(string) string {
	return func(s string) string {
		b, err := json.Marshal(value)
		if err != nil {
			panic(err)
		}
		return strings.Replace(s, goText, string(b), 1)
	}
}

func archInvalidCases() []archCase {
	base := archBase()
	valid := base.build()
	with := func(f func(p *archParts)) []byte {
		p := archBase()
		f(&p)
		return p.build()
	}
	schemaCase := func(data string) []byte {
		return with(func(p *archParts) { p.schemas[0].data = []byte(data) })
	}
	docCase := func(data string, docs int) []byte {
		return with(func(p *archParts) { p.entities[0].data = []byte(data); p.entities[0].docs = docs })
	}
	kvCase := func(data string, n int) []byte {
		return with(func(p *archParts) { p.kv = []byte(data); p.kvKeys = n })
	}
	rawManifest := func(raw string) []byte {
		return with(func(p *archParts) { p.raw = []byte(raw) })
	}
	tarOnly := func(t []byte) []byte { return archGzip(t, gzip.BestSpeed) }
	corrupt := append([]byte(nil), valid...)
	corrupt[len(corrupt)/2] ^= 0xff
	corrupt[len(corrupt)/2+1] ^= 0x5a
	badCRC := append([]byte(nil), valid...)
	badCRC[len(badCRC)-8] ^= 0xff
	badSize := append([]byte(nil), valid...)
	badSize[len(badSize)-4] ^= 0x01
	entityEntryEmpty := backup.EntityManifest{Name: "", Documents: 1}
	entityEntryNegative := backup.EntityManifest{Name: "books", Documents: -1}
	baseTar := base.tar()
	cut := bytes.Index(baseTar, []byte(`{"id":"b2"`))
	cases := []archCase{
		{slug: "not-gzip", data: []byte("this is not a gzip stream\n")},
		{slug: "empty-file", data: []byte{}},
		{slug: "gzip-header-truncated", data: valid[:5]},
		{slug: "gzip-stream-truncated", data: valid[:len(valid)*3/5]},
		{slug: "gzip-trailer-missing", accept: true, data: valid[:len(valid)-8]},
		{slug: "deflate-corrupt", data: corrupt},
		{slug: "gzip-bad-crc", accept: true, data: badCRC},
		{slug: "gzip-bad-size", accept: true, data: badSize},
		{slug: "tar-empty-stream", data: tarOnly(nil)},
		{slug: "tar-only-end-blocks", data: tarOnly(make([]byte, 1024))},
		{slug: "tar-garbage-header", data: tarOnly(bytes.Repeat([]byte("x"), 512))},
		{slug: "tar-truncated-in-entry", data: tarOnly(baseTar[:cut+5])},
		{slug: "manifest-not-first", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { e[0], e[1] = e[1], e[0]; return e }
		})},
		{slug: "manifest-not-regular", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { e[0].flag = tar.TypeDir; return e }
		})},
		{slug: "manifest-too-large", data: with(func(p *archParts) {
			p.raw = append([]byte("{"), bytes.Repeat([]byte(" "), 64<<20)...)
			p.raw = append(p.raw, '}')
			p.level = gzip.BestCompression
		})},
		{slug: "manifest-truncated-json", data: rawManifest(`{"format":`)},
		{slug: "manifest-trailing-data", data: rawManifest(`{} x`)},
		{slug: "manifest-float-version", data: rawManifest(`{"format":"elysiandb-backup","format_version":1.0}`)},
		{slug: "manifest-string-version", data: rawManifest(`{"format":"elysiandb-backup","format_version":"1"}`)},
		{slug: "manifest-array", data: rawManifest(`[]`)},
		{slug: "manifest-huge-number", data: rawManifest(`{"format":"elysiandb-backup","format_version":1e400}`)},
		{slug: "unknown-format", data: with(func(p *archParts) { p.edit = func(m *backup.Manifest) { m.Format = "other-backup" } })},
		{slug: "format-version-zero", data: with(func(p *archParts) { p.edit = func(m *backup.Manifest) { m.FormatVersion = 0 } })},
		{slug: "format-version-negative", data: with(func(p *archParts) { p.edit = func(m *backup.Manifest) { m.FormatVersion = -3 } })},
		{slug: "format-version-newer", data: with(func(p *archParts) { p.edit = func(m *backup.Manifest) { m.FormatVersion = 2 } })},
		{slug: "kv-count-negative", data: with(func(p *archParts) { p.edit = func(m *backup.Manifest) { m.KVKeys = -1 } })},
		{slug: "entity-entry-empty-name", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { m.Entities = append(m.Entities, entityEntryEmpty) }
		}), rule3: archRule3(fmt.Sprintf("%+v", entityEntryEmpty), entityEntryEmpty)},
		{slug: "entity-entry-negative", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { m.Entities[0] = entityEntryNegative }
		}), rule3: archRule3(fmt.Sprintf("%+v", entityEntryNegative), entityEntryNegative)},
		{slug: "entity-listed-twice", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { m.Entities = append(m.Entities, m.Entities[0]) }
		})},
		{slug: "schema-listed-twice", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { m.Schemas = append(m.Schemas, m.Schemas[0]) }
		})},
		{slug: "checksum-missing", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { delete(m.Files, backup.KVPath) }
		})},
		{slug: "checksum-unknown-file", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { m.Files["extra/file.json"] = archSHA(nil) }
		})},
		{slug: "checksum-unknown-files", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) {
				m.Files["zz/three.json"] = archSHA(nil)
				m.Files["aa/one.json"] = archSHA(nil)
				m.Files["mm/two.json"] = archSHA(nil)
			}
		}), d20: "checksum for an unknown file aa/one.json"},
		{slug: "key-missing", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { return e[:1] }
		})},
		{slug: "key-not-regular", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { e[1].flag = tar.TypeDir; return e }
		})},
		{slug: "key-wrong-name", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { e[1].name = "security/other.key"; return e }
		})},
		{slug: "key-too-large", data: with(func(p *archParts) { p.key = bytes.Repeat([]byte("a"), 64<<10+1) })},
		{slug: "key-blank", data: with(func(p *archParts) { p.key = []byte(" \n\t") })},
		{slug: "key-with-space", data: with(func(p *archParts) { p.key = []byte("abc def") })},
		{slug: "key-checksum-mismatch", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { m.Files[backup.KeyPath] = archSHA([]byte("other")) }
		})},
		{slug: "schema-not-object", data: schemaCase(`[1]`)},
		{slug: "schema-null", data: schemaCase(`null`)},
		{slug: "schema-garbage", data: schemaCase(`x`)},
		{slug: "schema-other-id", data: schemaCase(`{"id":"authors"}`)},
		{slug: "schema-number-id", data: schemaCase(`{"id":1}`), rule3: archRule3(fmt.Sprintf("%q", any(float64(1))), float64(1))},
		{slug: "schema-missing-id", data: schemaCase(`{"fields":{}}`), rule3: archRule3(fmt.Sprintf("%q", any(nil)), nil)},
		{slug: "schema-invalid-fields", data: schemaCase(`{"id":"books","fields":[]}`)},
		{slug: "schema-two-objects", data: schemaCase(`{"id":"books"}{"id":"books"}`)},
		{slug: "schema-missing-entry", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { return e[:2] }
		})},
		{slug: "entries-out-of-order", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { e[2], e[3] = e[3], e[2]; return e }
		})},
		{slug: "document-truncated-json", data: docCase(`{"id":"b1"`, 1)},
		{slug: "document-garbage", data: docCase(`x`, 1)},
		{slug: "document-array", data: docCase(`[1]`, 1)},
		{slug: "document-no-id", data: docCase(`{"a":1}`, 1)},
		{slug: "document-number-id", data: docCase(`{"id":1}`, 1)},
		{slug: "document-empty-id", data: docCase(`{"id":""}`, 1)},
		{slug: "document-duplicate-id", data: docCase(`{"id":"b1"}`+"\n"+`{"id":"b1"}`+"\n", 2)},
		{slug: "document-duplicate-escaped-id", data: docCase(`{"id":"b1"}`+"\n"+`{"i\u0064":"b1"}`+"\n", 2)},
		{slug: "document-trailing-garbage", data: docCase(`{"id":"b1"} ]`, 1)},
		{slug: "document-count-mismatch", data: with(func(p *archParts) { p.entities[0].docs = 3 })},
		{slug: "document-checksum-mismatch", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { m.Files[backup.EntityPath("books")] = archSHA([]byte("other")) }
		})},
		{slug: "entity-missing-entry", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { return e[:3] }
		})},
		{slug: "kv-unknown-field", data: kvCase(`{"key":"a","value":"b","x":1}`, 1)},
		{slug: "kv-key-number", data: kvCase(`{"key":1,"value":"b"}`, 1)},
		{slug: "kv-expiry-out-of-range", data: kvCase(`{"key":"a","value":"b","expires_at":1e400}`, 1)},
		{slug: "kv-expiry-float", data: kvCase(`{"key":"a","value":"b","expires_at":1.5}`, 1)},
		{slug: "kv-garbage", data: kvCase(`x`, 1)},
		{slug: "kv-array", data: kvCase(`[]`, 1)},
		{slug: "kv-empty-key", data: kvCase(`{"key":"","value":"b"}`, 1)},
		{slug: "kv-api-key", data: kvCase(`{"key":"api:entity:x","value":"b"}`, 1)},
		{slug: "kv-duplicate-key", data: kvCase(`{"key":"a","value":"b"}`+"\n"+`{"key":"a","value":"c"}`, 2)},
		{slug: "kv-negative-expiry", data: kvCase(`{"key":"a","value":"b","expires_at":-5}`, 1)},
		{slug: "kv-bad-base64", data: kvCase(`{"key":"a","value":"!!","encoding":"base64"}`, 1)},
		{slug: "kv-unknown-encoding", data: kvCase(`{"key":"a","value":"b","encoding":"hex"}`, 1)},
		{slug: "kv-count-mismatch", data: with(func(p *archParts) { p.kvKeys = 5 })},
		{slug: "kv-checksum-mismatch", data: with(func(p *archParts) {
			p.edit = func(m *backup.Manifest) { m.Files[backup.KVPath] = archSHA([]byte("other")) }
		})},
		{slug: "kv-missing-entry", data: with(func(p *archParts) {
			p.order = func(e []archEntry) []archEntry { return e[:4] }
		})},
		{slug: "unexpected-trailing-file", data: with(func(p *archParts) {
			p.extra = []archEntry{{name: "extra.json", data: []byte("{}")}}
		})},
		{slug: "unexpected-trailing-folder", data: with(func(p *archParts) {
			p.extra = []archEntry{{name: "extra/", flag: tar.TypeDir}}
		})},
	}
	return cases
}

func archWithCore(p *archParts) {
	seed, err := archSeed()
	if err != nil {
		panic(err)
	}
	p.schemas = append(p.schemas, seed.schemas...)
	p.entities = append(p.entities, seed.entities...)
}

func archValidCases() []archCase {
	with := func(f func(p *archParts)) []byte {
		p := archBase()
		f(&p)
		archWithCore(&p)
		return p.build()
	}
	twoMembers := func() []byte {
		p := archBase()
		archWithCore(&p)
		t := p.tar()
		return append(archGzip(t[:1000], gzip.BestSpeed), archGzip(t[1000:], gzip.BestSpeed)...)
	}
	nulFlag := func(t []byte) []byte {
		out := append([]byte(nil), t...)
		for off := 0; off+512 <= len(out); {
			h := out[off : off+512]
			if bytes.Equal(h, make([]byte, 512)) {
				break
			}
			size, _ := strconv.ParseInt(strings.Trim(string(h[124:136]), " \x00"), 8, 64)
			h[156] = 0
			for i := 148; i < 156; i++ {
				h[i] = ' '
			}
			sum := 0
			for _, c := range h {
				sum += int(c)
			}
			copy(h[148:156], fmt.Sprintf("%06o\x00 ", sum))
			off += 512 + int((size+511)/512*512)
		}
		return out
	}
	rawDocs := strings.Join([]string{
		`{"id":"fast","n":12345678901234567890,"t":"plain"}`,
		`{"id":"slow","n":12345678901234567890,"t":"\u00e9"}`,
		`{"i\u0064":"escaped-key","x":1}`,
		`{"id":"back\\slash","x":1}`,
		`{"id":"first","x":{"id":"inner"},"id":"last"}`,
		`{"id":"html","t":"<&>"}`,
		`{"id":"line-sep","t":"a` + "\u2028" + `b"}`,
		`{"id":"line-sep-escaped","t":"a\u2028b"}`,
		`{"id":"floats","a":1e21,"b":1.0,"c":-0,"d":0.000001,"e":1e-7,"f":123456789012345678}`,
		`{"id":"floats-slow","a":1e21,"b":1.0,"c":-0,"d":0.000001,"e":1e-7,"f":123456789012345678,"u":"\u0041"}`,
		`  {"id":"spaces"  ,  "x" : [ 1 , 2 ] }`,
		`{"id":"nested-escape","x":{"y":"\u0041"}}`,
		`{"x":1,"id":"id-last"}`,
		`{"id":"control","t":"tab\there"}`,
	}, "\n") + "\n"
	return []archCase{
		{slug: "two-gzip-members", data: twoMembers()},
		{slug: "typeflag-nul", data: with(func(p *archParts) { p.tarEdit = nulFlag })},
		{slug: "kv-case-insensitive", data: with(func(p *archParts) { p.kv = []byte(`{"KEY":"a","Value":"b"}` + "\n"); p.kvKeys = 1 })},
		{slug: "kv-expired-and-live", data: with(func(p *archParts) {
			p.kv = []byte(fmt.Sprintf(`{"key":"old","value":"x","expires_at":%d}`+"\n"+`{"key":"live","value":"y","expires_at":%d}`+"\n"+`{"key":"bin","value":"AP8=","encoding":"base64"}`+"\n", archOldExpiry, archLiveExpiry))
			p.kvKeys = 3
		})},
		{slug: "documents-without-separators", data: with(func(p *archParts) {
			p.entities[0].data = []byte(`{"id":"a"}{"id":"b"} {"id":"c"}`)
			p.entities[0].docs = 3
		})},
		{slug: "manifest-extra-keys", data: with(func(p *archParts) {
			p.rawEdit = func(b []byte) []byte {
				return bytes.Replace(b, []byte("{\n"), []byte("{\n  \"comment\": \"ignored\",\n  \"kv_keys\": 99,\n  \"Engine\": null,\n  \"CREATED_AT\": null,\n"), 1)
			}
		})},
		{slug: "raw-documents", data: with(func(p *archParts) {
			p.entities = append(p.entities, archEntity{name: "raw", data: []byte(rawDocs), docs: 14})
		})},
		{slug: "empty-entities", data: with(func(p *archParts) {
			p.entities = append(p.entities, archEntity{name: "none", data: nil, docs: 0})
			p.kv = nil
			p.kvKeys = 0
		})},
	}
}

func archEnc(row map[string]any, field string, b []byte) {
	if utf8.Valid(b) {
		row[field] = string(b)
		return
	}
	row[field+"_b64"] = base64.StdEncoding.EncodeToString(b)
}

func archMaskTar(t []byte) []byte {
	m := append([]byte(nil), t...)
	next := int64(-1)
	for off := 0; off+512 <= len(m); {
		h := m[off : off+512]
		if bytes.Equal(h, make([]byte, 512)) {
			break
		}
		flag := h[156]
		size, _ := strconv.ParseInt(strings.Trim(string(h[124:136]), " \x00"), 8, 64)
		if flag != tar.TypeXHeader && next >= 0 {
			size = next
			next = -1
		}
		name := string(bytes.TrimRight(h[0:100], "\x00"))
		for i := 136; i < 156; i++ {
			h[i] = 0
		}
		end := off + 512 + int(size)
		if end > len(m) {
			end = len(m)
		}
		data := m[off+512 : end]
		if flag == tar.TypeXHeader {
			for _, rec := range strings.Split(string(data), "\n") {
				if i := strings.Index(rec, " size="); i >= 0 {
					next, _ = strconv.ParseInt(rec[i+6:], 10, 64)
				}
			}
		}
		if name == backup.ManifestPath {
			for _, k := range []string{`"created_at": "`, `"elysiandb_version": "`} {
				if i := bytes.Index(data, []byte(k)); i >= 0 {
					for j := i + len(k); j < len(data) && data[j] != '"'; j++ {
						data[j] = 'X'
					}
				}
			}
		}
		off += 512 + int((size+511)/512*512)
	}
	return m
}

func archGunzip(b []byte) ([]byte, error) {
	gz, err := gzip.NewReader(bytes.NewReader(b))
	if err != nil {
		return nil, err
	}
	return io.ReadAll(gz)
}

func archEntries(t []byte) ([]string, error) {
	tr := tar.NewReader(bytes.NewReader(t))
	var names []string
	for {
		h, err := tr.Next()
		if err == io.EOF {
			return names, nil
		}
		if err != nil {
			return nil, err
		}
		names = append(names, h.Name)
	}
}

type archServer struct {
	cmd      *exec.Cmd
	logs     *lockedBuffer
	dir      string
	httpPort int
	tcpPort  int
	done     chan error
}

func archConfig(dir string, periodic bool) (int, int, error) {
	httpPort, tcpPort := freePort(), freePort()
	for tcpPort == 8088 || tcpPort == httpPort {
		tcpPort = freePort()
	}
	backupCfg := ""
	if periodic {
		backupCfg = "  backup: { folder: backups, everyMinutes: 1, keep: 3 }\n"
	}
	cfg := fmt.Sprintf("store:\n  folder: data\n  shards: 4\n%sserver:\n  http: { enabled: true, host: 127.0.0.1, port: %d }\n  tcp: { enabled: true, host: 127.0.0.1, port: %d }\napi:\n  schema:\n    enabled: true\nlog:\n  level: info\n  format: text\nsecurity:\n  authentication:\n    enabled: false\n", backupCfg, httpPort, tcpPort)
	return httpPort, tcpPort, os.WriteFile(filepath.Join(dir, "s.yaml"), []byte(cfg), 0o644)
}

func archStart(bin, dir string, httpPort, tcpPort int) (*archServer, error) {
	s := &archServer{logs: &lockedBuffer{}, dir: dir, httpPort: httpPort, tcpPort: tcpPort, done: make(chan error, 1)}
	s.cmd = exec.Command(bin, "-config", "s.yaml")
	s.cmd.Dir = dir
	s.cmd.Env = []string{"PATH=" + os.Getenv("PATH"), "HOME=" + dir, "TZ=UTC"}
	s.cmd.Stdout = s.logs
	s.cmd.Stderr = s.logs
	if err := s.cmd.Start(); err != nil {
		return nil, err
	}
	go func() { s.done <- s.cmd.Wait() }()
	for i := 0; i < 200; i++ {
		if c, err := net.Dial("tcp4", "127.0.0.1:"+strconv.Itoa(httpPort)); err == nil {
			c.Close()
			time.Sleep(200 * time.Millisecond)
			return s, nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	s.stop()
	return nil, fmt.Errorf("server did not start: %s", s.logs.take())
}

func (s *archServer) stop() {
	s.cmd.Process.Signal(os.Interrupt)
	select {
	case <-s.done:
	case <-time.After(20 * time.Second):
		s.cmd.Process.Kill()
		<-s.done
	}
}

func (s *archServer) http(method, path, body string) (int, []byte, error) {
	req, err := http.NewRequest(method, "http://127.0.0.1:"+strconv.Itoa(s.httpPort)+path, strings.NewReader(body))
	if err != nil {
		return 0, nil, err
	}
	if body != "" {
		req.Header.Set("Content-Type", "application/json")
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return 0, nil, err
	}
	defer resp.Body.Close()
	b, err := io.ReadAll(resp.Body)
	return resp.StatusCode, b, err
}

func (s *archServer) tcp(payload []byte) ([]byte, error) {
	conn, err := net.Dial("tcp4", "127.0.0.1:"+strconv.Itoa(s.tcpPort))
	if err != nil {
		return nil, err
	}
	defer conn.Close()
	if _, err := conn.Write(payload); err != nil {
		return nil, err
	}
	recv, _ := readQuiet(conn, 250*time.Millisecond, 5*time.Second)
	return []byte(recv), nil
}

type archStep struct {
	op     string
	method string
	path   string
	body   string
	send   string
	ms     int
	repeat *archRepeat
}

type archRepeat struct {
	count  int
	prefix string
	width  int
	head   string
	mid    string
	item   string
	items  int
	tail   string
}

func (r *archRepeat) body(i int) string {
	return r.head + r.prefix + fmt.Sprintf("%0*d", r.width, i) + r.mid + strings.Repeat(r.item, r.items) + r.tail
}

func archRun(s *archServer, dataset string, steps []archStep, recipe *goldenio.Lines) error {
	for i, st := range steps {
		row := map[string]any{"dataset": dataset, "seq": i + 1, "op": st.op}
		switch st.op {
		case "http":
			status, _, err := s.http(st.method, st.path, st.body)
			if err != nil {
				return err
			}
			row["method"], row["path"], row["body"], row["status"] = st.method, st.path, st.body, status
		case "tcp":
			reply, err := s.tcp([]byte(st.send))
			if err != nil {
				return err
			}
			archEnc(row, "send", []byte(st.send))
			archEnc(row, "reply", reply)
		case "sleep":
			time.Sleep(time.Duration(st.ms) * time.Millisecond)
			row["ms"] = st.ms
		case "http_repeat":
			r := st.repeat
			statuses := map[string]int{}
			for n := 0; n < r.count; n++ {
				status, _, err := s.http(st.method, st.path, r.body(n))
				if err != nil {
					return err
				}
				statuses[strconv.Itoa(status)]++
			}
			row["method"], row["path"], row["count"], row["id_prefix"], row["id_width"] = st.method, st.path, r.count, r.prefix, r.width
			row["body_head"], row["body_mid"], row["item"], row["items"], row["body_tail"], row["statuses"] = r.head, r.mid, r.item, r.items, r.tail, statuses
		}
		recipe.Add(row)
	}
	return nil
}

func archAPISteps() []archStep {
	post := func(path, body string) archStep { return archStep{op: "http", method: "POST", path: path, body: body} }
	tcp := func(send string) archStep { return archStep{op: "tcp", send: send} }
	return []archStep{
		post("/api/books", `{"id":"f1","title":"plain","n":1}`),
		post("/api/books", `{"id":"s1","title":"<b> & </b>","n":1e21,"small":1.5e-7,"neg":-0.0}`),
		post("/api/books", `{"id":"s2","line":"a`+"\u2028"+`b","big":12345678901234567890,"accent":"\u00e9t\u00e9"}`),
		post("/api/books", `{"id":"n1","nested":{"a":[1,2.5,{"b":"c"}],"empty":{},"nil":null,"t":true}}`),
		post("/api/books", `{"id":"q\"1","title":"quote in id"}`),
		post("/api/authors/create", `{"fields":{"name":{"type":"string","required":true},"age":{"type":"number"}}}`),
		post("/api/authors", `{"id":"a1","name":"Ann","age":40}`),
		post("/api/x:y", `{"id":"1"}`),
		post("/api/my%20books", `{"id":"1"}`),
		post("/api/a+b@c!~", `{"id":"1"}`),
		post("/api/"+strings.Repeat("u", 90), `{"id":"1"}`),
		post("/api/"+strings.Repeat("p", 120), `{"id":"1"}`),
		post("/api/"+strings.Repeat("q", 300), `{"id":"1"}`),
		tcp("SET plain value\r\n"),
		tcp("SET spaced  two  spaces \r\n"),
		tcp("SET ctl a\x01b\x7fc\td\x1fe\r\n"),
		tcp("SET bin \xff\xfe\x00z\r\n"),
		tcp("SET bad\xffkey v\r\n"),
		tcp("SET html <&>\"\\\r\n"),
		tcp("SET emptyval \r\n"),
		tcp("SET TTL=1 gone v\r\n"),
		{op: "sleep", ms: 2100},
		tcp("GET gone\r\n"),
	}
}

func archLargeSteps() []archStep {
	return []archStep{{op: "http_repeat", method: "POST", path: "/api/big", repeat: &archRepeat{
		count: 20, prefix: "big-", width: 2, head: `{"id":"`, mid: `","t":"<","items":[`, item: `{"a":1,"b":"xyz"},`, items: 3000, tail: `{"a":1}]}`,
	}}}
}

func archHot(s *archServer, beats bool) ([]byte, string, error) {
	type result struct {
		body []byte
		name string
		err  error
	}
	ch := make(chan result, 1)
	go func() {
		resp, err := http.Get("http://127.0.0.1:" + strconv.Itoa(s.httpPort) + "/api/backup")
		if err != nil {
			ch <- result{err: err}
			return
		}
		defer resp.Body.Close()
		b, err := io.ReadAll(resp.Body)
		name := resp.Header.Get("Content-Disposition")
		if resp.StatusCode != 200 {
			err = fmt.Errorf("backup status %d: %s", resp.StatusCode, b)
		}
		ch <- result{body: b, name: name, err: err}
	}()
	if !beats {
		r := <-ch
		return r.body, r.name, r.err
	}
	for {
		select {
		case r := <-ch:
			return r.body, r.name, r.err
		case <-time.After(15 * time.Millisecond):
		}
		s.cmd.Process.Signal(syscall.SIGSTOP)
		time.Sleep(1100 * time.Millisecond)
		s.cmd.Process.Signal(syscall.SIGCONT)
	}
}

func archCLI(bin, dir string, args ...string) (string, error) {
	cmd := exec.Command(bin, args...)
	cmd.Dir = dir
	cmd.Env = []string{"PATH=" + os.Getenv("PATH"), "HOME=" + dir, "TZ=UTC", "NO_COLOR=1"}
	out, err := cmd.CombinedOutput()
	if err != nil {
		return string(out), fmt.Errorf("%v: %v: %s", args, err, out)
	}
	return string(out), nil
}

func archSingle(dir string) ([]byte, error) {
	matches, err := filepath.Glob(filepath.Join(dir, "elysiandb-backup-*.tar.gz"))
	if err != nil {
		return nil, err
	}
	if len(matches) != 1 {
		return nil, fmt.Errorf("expected one archive in %s, found %d", dir, len(matches))
	}
	return os.ReadFile(matches[0])
}

type archBuilt struct {
	dataset string
	kind    string
	gz      []byte
	name    string
}

func archDataset(bin, base, dataset string, seed []byte, steps []archStep, kinds []string, recipe *goldenio.Lines) ([]archBuilt, error) {
	dir := filepath.Join(base, "dataset-"+dataset)
	if err := os.MkdirAll(filepath.Join(dir, "out"), 0o755); err != nil {
		return nil, err
	}
	periodic := false
	for _, k := range kinds {
		periodic = periodic || k == "periodic"
	}
	httpPort, tcpPort, err := archConfig(dir, periodic)
	if err != nil {
		return nil, err
	}
	if err := os.WriteFile(filepath.Join(dir, "seed.tar.gz"), seed, 0o644); err != nil {
		return nil, err
	}
	if _, err := archCLI(bin, dir, "-config", "s.yaml", "restore", "seed.tar.gz", "--force"); err != nil {
		return nil, err
	}
	recipe.Add(map[string]any{"dataset": dataset, "seq": 0, "op": "restore", "file": "seed/seed.tar.gz", "args": []string{"restore", "<file>", "--force"}})
	started := time.Now()
	s, err := archStart(bin, dir, httpPort, tcpPort)
	if err != nil {
		return nil, err
	}
	var built []archBuilt
	fail := func(err error) ([]archBuilt, error) {
		s.stop()
		return nil, err
	}
	if err := archRun(s, dataset, steps, recipe); err != nil {
		return fail(err)
	}
	for _, kind := range kinds {
		switch kind {
		case "periodic":
			deadline := started.Add(100 * time.Second)
			for {
				matches, _ := filepath.Glob(filepath.Join(dir, "backups", "elysiandb-backup-*.tar.gz"))
				if len(matches) > 0 {
					time.Sleep(500 * time.Millisecond)
					b, err := os.ReadFile(matches[0])
					if err != nil {
						return fail(err)
					}
					built = append(built, archBuilt{dataset: dataset, kind: kind, gz: b, name: filepath.Base(matches[0])})
					break
				}
				if time.Now().After(deadline) {
					return fail(fmt.Errorf("no periodic backup: %s", s.logs.take()))
				}
				time.Sleep(500 * time.Millisecond)
			}
		case "hot", "hot-beats":
			b, name, err := archHot(s, kind == "hot-beats")
			if err != nil {
				return fail(err)
			}
			built = append(built, archBuilt{dataset: dataset, kind: kind, gz: b, name: name})
		}
	}
	s.stop()
	for _, kind := range kinds {
		if kind != "backup" {
			continue
		}
		if _, err := archCLI(bin, dir, "-config", "s.yaml", "backup", "--output", "out"); err != nil {
			return nil, err
		}
		b, err := archSingle(filepath.Join(dir, "out"))
		if err != nil {
			return nil, err
		}
		built = append(built, archBuilt{dataset: dataset, kind: kind, gz: b})
	}
	return built, nil
}

func archRestoreRow(bin, base, file string, data []byte) (map[string]any, []byte, error) {
	dir, err := os.MkdirTemp(base, "restore-")
	if err != nil {
		return nil, nil, err
	}
	if err := os.MkdirAll(filepath.Join(dir, "out"), 0o755); err != nil {
		return nil, nil, err
	}
	if _, _, err := archConfig(dir, false); err != nil {
		return nil, nil, err
	}
	if err := os.WriteFile(filepath.Join(dir, "archive.tar.gz"), data, 0o644); err != nil {
		return nil, nil, err
	}
	row, err := runCLI(bin, dir, cliCase{name: "restore " + file, args: []string{"-config", "s.yaml", "restore", "archive.tar.gz", "--force"}})
	if err != nil {
		return nil, nil, err
	}
	row["file"] = file
	if row["exit"] != 0 {
		return row, nil, nil
	}
	if _, err := archCLI(bin, dir, "-config", "s.yaml", "backup", "--output", "out"); err != nil {
		return nil, nil, err
	}
	again, err := archSingle(filepath.Join(dir, "out"))
	if err != nil {
		return nil, nil, err
	}
	t, err := archGunzip(again)
	return row, t, err
}

func archVectors(out string) error {
	var l goldenio.Lines
	tarVector := func(kind, name string, size int64, data []byte) error {
		var buf bytes.Buffer
		tw := tar.NewWriter(&buf)
		if err := tw.WriteHeader(&tar.Header{Name: name, Mode: 0o600, Size: size, ModTime: archTime, Typeflag: tar.TypeReg}); err != nil {
			return err
		}
		row := map[string]any{"kind": kind, "name": name, "name_len": len(name), "size": size, "mtime": archTime.Format(time.RFC3339)}
		if data == nil {
			row["header_hex"] = hex.EncodeToString(buf.Bytes())
			l.Add(row)
			return nil
		}
		if _, err := tw.Write(data); err != nil {
			return err
		}
		if err := tw.Close(); err != nil {
			return err
		}
		row["data"] = string(data)
		row["archive_hex"] = hex.EncodeToString(buf.Bytes())
		row["archive_len"] = buf.Len()
		l.Add(row)
		return nil
	}
	if err := tarVector("ustar", "manifest.json", 5, []byte("hello")); err != nil {
		return err
	}
	for _, n := range []int{100, 101, 105, 109} {
		name := "entities/" + strings.Repeat("u", n-len("entities/")-len(".jsonl")) + ".jsonl"
		if err := tarVector("ustar-long", name, 1, []byte("x")); err != nil {
			return err
		}
	}
	for _, n := range []int{110, 165, 315} {
		name := "entities/" + strings.Repeat("p", n-len("entities/")-len(".jsonl")) + ".jsonl"
		if err := tarVector("pax-path", name, 1, []byte("x")); err != nil {
			return err
		}
	}
	if err := tarVector("pax-size", "entities/big.jsonl", 10<<30, nil); err != nil {
		return err
	}
	if err := tarVector("pax-path-and-size", "entities/"+strings.Repeat("s", 120)+".jsonl", 10<<30, nil); err != nil {
		return err
	}
	gzVector := func(kind string, f func(gz *gzip.Writer, buf *bytes.Buffer) map[string]any) {
		var buf bytes.Buffer
		gz, _ := gzip.NewWriterLevel(&buf, gzip.BestSpeed)
		row := f(gz, &buf)
		row["kind"] = kind
		row["level"] = gzip.BestSpeed
		l.Add(row)
	}
	gzVector("gzip-x", func(gz *gzip.Writer, buf *bytes.Buffer) map[string]any {
		gz.Write([]byte("x"))
		gz.Close()
		return map[string]any{"input": "x", "output_hex": hex.EncodeToString(buf.Bytes())}
	})
	gzVector("gzip-empty", func(gz *gzip.Writer, buf *bytes.Buffer) map[string]any {
		gz.Close()
		return map[string]any{"input": "", "output_hex": hex.EncodeToString(buf.Bytes())}
	})
	gzVector("gzip-empty-flush", func(gz *gzip.Writer, buf *bytes.Buffer) map[string]any {
		gz.Flush()
		flushed := hex.EncodeToString(buf.Bytes())
		gz.Close()
		return map[string]any{"input": "", "flushed_hex": flushed, "output_hex": hex.EncodeToString(buf.Bytes())}
	})
	gzVector("gzip-flush-between", func(gz *gzip.Writer, buf *bytes.Buffer) map[string]any {
		gz.Write([]byte("abc"))
		gz.Flush()
		gz.Flush()
		gz.Write([]byte("abcabc"))
		gz.Close()
		return map[string]any{"writes": []string{"abc", "<flush>", "<flush>", "abcabc"}, "output_hex": hex.EncodeToString(buf.Bytes())}
	})
	mixed := archMixed(1 << 20)
	if err := goldenio.WriteFile(out, "archives/vectors/mixed.bin", mixed); err != nil {
		return err
	}
	single := archGzip(mixed, gzip.BestSpeed)
	if err := goldenio.WriteFile(out, "archives/vectors/mixed.gz", single); err != nil {
		return err
	}
	var flushed bytes.Buffer
	gz, _ := gzip.NewWriterLevel(&flushed, gzip.BestSpeed)
	gz.Flush()
	for off := 0; off < len(mixed); off += 65536 {
		gz.Write(mixed[off:min(off+65536, len(mixed))])
		gz.Flush()
	}
	gz.Close()
	if err := goldenio.WriteFile(out, "archives/vectors/mixed-flush.gz", flushed.Bytes()); err != nil {
		return err
	}
	l.Add(map[string]any{"kind": "gzip-mixed", "level": gzip.BestSpeed, "input": "vectors/mixed.bin", "input_sha256": archSHA(mixed), "output": "vectors/mixed.gz", "output_sha256": archSHA(single), "writes": "one write"})
	l.Add(map[string]any{"kind": "gzip-mixed-flush", "level": gzip.BestSpeed, "input": "vectors/mixed.bin", "input_sha256": archSHA(mixed), "output": "vectors/mixed-flush.gz", "output_sha256": archSHA(flushed.Bytes()), "writes": "flush, then each 65536-byte slice followed by a flush, then close"})
	return l.Write(out, "archives/vectors.jsonl")
}

func archMixed(n int) []byte {
	out := make([]byte, 0, n)
	var x uint64 = 0x9e3779b97f4a7c15
	next := func() uint64 {
		x ^= x >> 12
		x ^= x << 25
		x ^= x >> 27
		return x * 0x2545f4914f6cdd1d
	}
	for block := 0; len(out) < n; block++ {
		var chunk []byte
		switch block % 4 {
		case 0:
			chunk = make([]byte, 4096)
			for i := range chunk {
				chunk[i] = byte(next() >> 56)
			}
		case 1:
			chunk = bytes.Repeat([]byte("elysiandb "), 410)[:4096]
		case 2:
			var b bytes.Buffer
			for b.Len() < 4096 {
				fmt.Fprintf(&b, `{"id":"%d","n":%d}`+"\n", block, next()%1000)
			}
			chunk = b.Bytes()[:4096]
		default:
			chunk = make([]byte, 4096)
		}
		out = append(out, chunk...)
	}
	return out[:n]
}

func genArchives(out string) error {
	globals.VERSION = "0.3.0"
	base, err := os.MkdirTemp("", "elysiandb-golden-archives-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(base)
	bin := filepath.Join(base, "elysiandb")
	build := exec.Command("go", "build", "-ldflags", "-X github.com/taymour/elysiandb/internal/globals.VERSION=0.3.0", "-o", bin, ".")
	build.Stderr = os.Stderr
	if err := build.Run(); err != nil {
		return err
	}
	var expected goldenio.Lines
	var accepted []string
	for _, c := range archInvalidCases() {
		file := "invalid/" + c.slug + ".tar.gz"
		reason, values, failed := archMaskedReason(c.data)
		if c.d20 != "" {
			for i := 0; failed && !strings.HasSuffix(reason, c.d20) && i < 2000; i++ {
				reason, values, failed = archMaskedReason(c.data)
			}
			if !strings.HasSuffix(reason, c.d20) {
				return fmt.Errorf("%s: no sorted pick: %s", c.slug, reason)
			}
		}
		if !failed && c.accept {
			file = "valid/" + c.slug + ".tar.gz"
			accepted = append(accepted, file)
			expected.Add(map[string]any{"file": file, "reason": nil, "note": "accepted: the gzip trailer is never read once the tar end blocks are read"})
			if err := goldenio.WriteFile(out, "archives/"+file, c.data); err != nil {
				return err
			}
			continue
		}
		if !failed {
			return fmt.Errorf("%s: the archive verified", c.slug)
		}
		row := map[string]any{"file": file, "reason": reason}
		if c.rule3 != nil {
			row["go_reason"] = reason
			row["reason"] = c.rule3(reason)
			if row["reason"] == reason {
				return fmt.Errorf("%s: rule 3 did not apply: %s", c.slug, reason)
			}
		}
		if c.d20 != "" {
			row["order"] = "first unknown path in sorted order (D20)"
		}
		if len(values) > 0 {
			row["go_values"] = values
		}
		if strings.Contains(reason, "supported by ElysianDB") {
			row["version"] = globals.VERSION
		}
		expected.Add(row)
		if err := goldenio.WriteFile(out, "archives/"+file, c.data); err != nil {
			return err
		}
	}
	var restores goldenio.Lines
	var rebackups []map[string]any
	for _, c := range archValidCases() {
		file := "valid/" + c.slug + ".tar.gz"
		if reason, _, failed := archMaskedReason(c.data); failed {
			return fmt.Errorf("%s: %s", c.slug, reason)
		}
		expected.Add(map[string]any{"file": file, "reason": nil})
		if err := goldenio.WriteFile(out, "archives/"+file, c.data); err != nil {
			return err
		}
	}
	if err := expected.Write(out, "archives/expected.jsonl"); err != nil {
		return err
	}
	if err := archVectors(out); err != nil {
		return err
	}
	seedParts, err := archSeed()
	if err != nil {
		return err
	}
	seed := seedParts.build()
	if err := goldenio.WriteFile(out, "archives/seed/seed.tar.gz", seed); err != nil {
		return err
	}
	if err := goldenio.WriteFile(out, "archives/seed/seed.tar", seedParts.tar()); err != nil {
		return err
	}
	var recipe goldenio.Lines
	var built []archBuilt
	for _, d := range []struct {
		name  string
		steps []archStep
		kinds []string
	}{
		{"api", archAPISteps(), []string{"periodic", "hot", "backup"}},
		{"seed-only", nil, []string{"hot", "backup"}},
		{"large", archLargeSteps(), []string{"hot-beats", "backup"}},
	} {
		b, err := archDataset(bin, base, d.name, seed, d.steps, d.kinds, &recipe)
		if err != nil {
			return fmt.Errorf("%s: %w", d.name, err)
		}
		built = append(built, b...)
	}
	if err := recipe.Write(out, "archives/recipe.jsonl"); err != nil {
		return err
	}
	var index goldenio.Lines
	reference := map[string][]byte{}
	for _, b := range built {
		t, err := archGunzip(b.gz)
		if err != nil {
			return fmt.Errorf("%s/%s: %w", b.dataset, b.kind, err)
		}
		masked := archMaskTar(t)
		if ref, ok := reference[b.dataset]; ok && !bytes.Equal(ref, masked) {
			return fmt.Errorf("%s/%s: the tar differs from the first archive of the data set", b.dataset, b.kind)
		}
		reference[b.dataset] = masked
		names, err := archEntries(t)
		if err != nil {
			return err
		}
		file := b.dataset + "/" + b.kind + ".tar.gz"
		regzip := archGzip(t, gzip.BestSpeed)
		row := map[string]any{"dataset": b.dataset, "kind": b.kind, "file": file, "tar": b.dataset + "/" + b.kind + ".tar", "tar_size": len(t), "tar_sha256": archSHA(t), "gzip_size": len(b.gz), "entries": names, "same_as_single_write_gzip": bytes.Equal(regzip, b.gz)}
		if b.kind == "hot-beats" && bytes.Equal(regzip, b.gz) {
			return fmt.Errorf("%s: no sync flush in the hot backup", b.dataset)
		}
		if (b.kind == "backup" || b.kind == "periodic") && !bytes.Equal(regzip, b.gz) {
			return fmt.Errorf("%s/%s: the archive is not a single-write gzip", b.dataset, b.kind)
		}
		if b.name != "" {
			row["served_name"] = cliStampRe.ReplaceAllString(b.name, "<STAMP>")
		}
		index.Add(row)
		if err := goldenio.WriteFile(out, "archives/"+file, b.gz); err != nil {
			return err
		}
		if err := goldenio.WriteFile(out, "archives/"+b.dataset+"/"+b.kind+".tar", t); err != nil {
			return err
		}
	}
	if err := index.Write(out, "archives/archives.jsonl"); err != nil {
		return err
	}
	var files []string
	files = append(files, "seed/seed.tar.gz")
	for _, b := range built {
		files = append(files, b.dataset+"/"+b.kind+".tar.gz")
	}
	for _, c := range archValidCases() {
		files = append(files, "valid/"+c.slug+".tar.gz")
	}
	files = append(files, accepted...)
	for _, f := range files {
		data, err := os.ReadFile(filepath.Join(out, "archives", f))
		if err != nil {
			return err
		}
		row, again, err := archRestoreRow(bin, base, f, data)
		if err != nil {
			return fmt.Errorf("restore %s: %w", f, err)
		}
		if again != nil {
			original, err := archGunzip(data)
			if err != nil && !strings.HasPrefix(f, "valid/gzip-") {
				return fmt.Errorf("%s: %w", f, err)
			}
			row["rebackup_equal"] = bytes.Equal(archMaskTar(original), archMaskTar(again))
			if strings.HasPrefix(f, "valid/") && !strings.HasPrefix(f, "valid/gzip-") {
				rel := strings.TrimSuffix(f, ".tar.gz") + ".rebackup.tar"
				if err := goldenio.WriteFile(out, "archives/"+rel, again); err != nil {
					return err
				}
				row["rebackup"] = rel
			}
		}
		rebackups = append(rebackups, row)
	}
	for _, r := range rebackups {
		restores.Add(r)
	}
	return restores.Write(out, "archives/restore.jsonl")
}
