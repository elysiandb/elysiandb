package legacy

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"
)

const (
	DataFile          = "elysiandb.json"
	JsonDataFile      = "elysiandbjson.json"
	ExpirationFile    = "elysiandb.expiration.json"
	JsonRecoveryFile  = "elysiandb.json.recovery.log"
	StoreRecoveryFile = "elysiandb.store.recovery.log"
	FailedSuffix      = ".failed"
	ApiKeyPrefix      = "api:entity:"
	TypesListKey      = "api:entity:types:list"
)

var Files = []string{DataFile, JsonDataFile, ExpirationFile, JsonRecoveryFile, StoreRecoveryFile, JsonRecoveryFile + FailedSuffix, StoreRecoveryFile + FailedSuffix}

type Document struct {
	ID  string
	Doc map[string]any
}

type Data struct {
	KV          map[string][]byte
	Expirations map[string]int64
	Types       []string
	Entities    map[string][]Document
	Replayed    int
}

func IDIndexKey(entity string) string {
	return ApiKeyPrefix + entity + ":internal:index:id"
}

func DocumentKey(entity, id string) string {
	return ApiKeyPrefix + entity + ":id:" + id
}

func Present(folder string) bool {
	for _, name := range Files {
		if _, err := os.Stat(filepath.Join(folder, name)); err == nil {
			return true
		}
	}

	return false
}

func FilesIn(folder string) []string {
	out := []string{}
	for _, name := range Files {
		if _, err := os.Stat(filepath.Join(folder, name)); err == nil {
			out = append(out, name)
		}
	}

	return out
}

func readJSONFile(path string, out any) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	if len(bytes.TrimSpace(data)) == 0 {
		return nil
	}

	return json.Unmarshal(data, out)
}

func ReadKVFile(path string) (map[string][]byte, error) {
	kv := map[string][]byte{}
	if err := readJSONFile(path, &kv); err != nil {
		return nil, err
	}

	return kv, nil
}

func ReadExpirationsFile(path string) (map[int64][]string, error) {
	raw := map[string][]string{}
	if err := readJSONFile(path, &raw); err != nil {
		return nil, err
	}

	out := make(map[int64][]string, len(raw))
	for k, list := range raw {
		ts, err := strconv.ParseInt(k, 10, 64)
		if err != nil {
			continue
		}

		out[ts] = append([]string(nil), list...)
	}

	return out, nil
}

func ReadDocumentsFile(path string) (map[string]map[string]any, error) {
	docs := map[string]map[string]any{}
	if err := readJSONFile(path, &docs); err != nil {
		return nil, err
	}

	return docs, nil
}

func readIfPresent[T any](path string, read func(string) (T, error), empty T) (T, error) {
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return empty, nil
	}

	v, err := read(path)
	if err != nil {
		return empty, fmt.Errorf("%s: %w", filepath.Base(path), err)
	}

	return v, nil
}

func Read(folder string, replayRecovery bool, now time.Time) (*Data, error) {
	kv, err := readIfPresent(filepath.Join(folder, DataFile), ReadKVFile, map[string][]byte{})
	if err != nil {
		return nil, err
	}

	rawExp, err := readIfPresent(filepath.Join(folder, ExpirationFile), ReadExpirationsFile, map[int64][]string{})
	if err != nil {
		return nil, err
	}

	docs, err := readIfPresent(filepath.Join(folder, JsonDataFile), ReadDocumentsFile, map[string]map[string]any{})
	if err != nil {
		return nil, err
	}

	expirations := map[string]int64{}
	for at, keys := range rawExp {
		for _, k := range keys {
			expirations[k] = at
		}
	}

	nowUnix := now.Unix()
	for k, at := range expirations {
		if at < nowUnix {
			delete(kv, k)
			delete(expirations, k)
		}
	}

	d := &Data{KV: kv, Expirations: expirations}

	if replayRecovery {
		n, err := replayJSONLog(filepath.Join(folder, JsonRecoveryFile), docs)
		if err != nil {
			return nil, err
		}
		d.Replayed += n

		n, err = replayStoreLog(filepath.Join(folder, StoreRecoveryFile), kv, expirations, nowUnix)
		if err != nil {
			return nil, err
		}
		d.Replayed += n
	}

	d.Types, d.Entities = repair(kv, docs)

	return d, nil
}

func eachLine(path string, fn func(line []byte)) error {
	f, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}

		return err
	}

	defer f.Close()

	r := bufio.NewReader(f)
	for {
		line, err := r.ReadBytes('\n')
		if trimmed := bytes.TrimSpace(line); len(trimmed) > 0 {
			fn(trimmed)
		}

		if err == io.EOF {
			return nil
		}

		if err != nil {
			return err
		}
	}
}

func replayJSONLog(path string, docs map[string]map[string]any) (int, error) {
	type op struct {
		Op    string         `json:"op"`
		Key   string         `json:"key"`
		Value map[string]any `json:"value,omitempty"`
	}

	n := 0
	err := eachLine(path, func(line []byte) {
		var o op
		if json.Unmarshal(line, &o) != nil {
			return
		}

		switch o.Op {
		case "put":
			docs[o.Key] = o.Value
			n++
		case "del":
			delete(docs, o.Key)
			n++
		}
	})

	return n, err
}

func replayStoreLog(path string, kv map[string][]byte, expirations map[string]int64, now int64) (int, error) {
	type op struct {
		Op    string `json:"op"`
		Key   string `json:"key"`
		Value []byte `json:"value,omitempty"`
		TTL   int    `json:"ttl,omitempty"`
	}

	n := 0
	err := eachLine(path, func(line []byte) {
		var o op
		if json.Unmarshal(line, &o) != nil {
			return
		}

		switch o.Op {
		case "put":
			if o.TTL > 0 && now >= int64(o.TTL) {
				return
			}

			kv[o.Key] = o.Value
			if o.TTL > 0 {
				expirations[o.Key] = int64(o.TTL)
			}
			n++
		case "del":
			delete(kv, o.Key)
			delete(expirations, o.Key)
			n++
		}
	})

	return n, err
}

func decodeIDs(b []byte) []string {
	out := []string{}
	for _, p := range bytes.Split(b, []byte{'\n'}) {
		if len(p) > 0 {
			out = append(out, string(p))
		}
	}

	return out
}

func parseDocumentKey(key string) (string, string, bool) {
	rest, ok := strings.CutPrefix(key, ApiKeyPrefix)
	if !ok {
		return "", "", false
	}

	entity, id, ok := strings.Cut(rest, ":id:")
	if !ok || entity == "" || id == "" {
		return "", "", false
	}

	return entity, id, true
}

func repair(kv map[string][]byte, docs map[string]map[string]any) ([]string, map[string][]Document) {
	byEntity := map[string]map[string]map[string]any{}
	for key, doc := range docs {
		entity, id, ok := parseDocumentKey(key)
		if !ok {
			continue
		}

		if byEntity[entity] == nil {
			byEntity[entity] = map[string]map[string]any{}
		}
		byEntity[entity][id] = doc
	}

	types := decodeIDs(kv[TypesListKey])
	known := map[string]bool{}
	for _, t := range types {
		known[t] = true
	}

	for _, entity := range slices.Sorted(maps.Keys(byEntity)) {
		if !known[entity] {
			types = append(types, entity)
			known[entity] = true
		}
	}

	out := map[string][]Document{}
	for _, entity := range types {
		present := byEntity[entity]
		seen := map[string]bool{}
		list := []Document{}
		for _, id := range decodeIDs(kv[IDIndexKey(entity)]) {
			doc, ok := present[id]
			if !ok || seen[id] {
				continue
			}

			seen[id] = true
			list = append(list, Document{ID: id, Doc: doc})
		}

		missing := []string{}
		for id := range present {
			if !seen[id] {
				missing = append(missing, id)
			}
		}
		sort.Strings(missing)

		for _, id := range missing {
			list = append(list, Document{ID: id, Doc: present[id]})
		}

		out[entity] = list
	}

	return dedupe(types), out
}

func dedupe(list []string) []string {
	seen := map[string]bool{}
	out := make([]string, 0, len(list))
	for _, v := range list {
		if !seen[v] {
			seen[v] = true
			out = append(out, v)
		}
	}

	return out
}

func Write(folder string, d *Data) error {
	if err := os.MkdirAll(folder, 0o755); err != nil {
		return err
	}

	kv := map[string][]byte{}
	for k, v := range d.KV {
		if !strings.HasPrefix(k, ApiKeyPrefix) {
			kv[k] = v
		}
	}

	if len(d.Types) > 0 {
		kv[TypesListKey] = []byte(strings.Join(d.Types, "\n"))
	}

	docs := map[string]map[string]any{}
	for _, entity := range d.Types {
		ids := make([]string, 0, len(d.Entities[entity]))
		for _, doc := range d.Entities[entity] {
			ids = append(ids, doc.ID)
			docs[DocumentKey(entity, doc.ID)] = doc.Doc
		}

		if len(ids) > 0 {
			kv[IDIndexKey(entity)] = []byte(strings.Join(ids, "\n"))
		}
	}

	exp := map[string][]string{}
	for k, at := range d.Expirations {
		if _, ok := kv[k]; ok {
			ts := strconv.FormatInt(at, 10)
			exp[ts] = append(exp[ts], k)
		}
	}

	for _, keys := range exp {
		sort.Strings(keys)
	}

	for name, value := range map[string]any{DataFile: kv, JsonDataFile: docs, ExpirationFile: exp} {
		if err := writeJSON(filepath.Join(folder, name), value); err != nil {
			return err
		}
	}

	return nil
}

func writeJSON(path string, value any) error {
	tmp := path + ".tmp"
	f, err := os.OpenFile(tmp, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}

	err = json.NewEncoder(f).Encode(value)
	if err == nil {
		err = f.Sync()
	}

	if closeErr := f.Close(); err == nil {
		err = closeErr
	}

	if err != nil {
		_ = os.Remove(tmp)
		return err
	}

	return os.Rename(tmp, path)
}
