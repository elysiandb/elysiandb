package api_storage

import (
	"encoding/json"
	"io"

	"github.com/taymour/elysiandb/internal/datastore"
)

const (
	restoreBatch      = 1000
	rawBatchDocuments = 2048
	rawBatchBytes     = 8 << 20
	rawBatchWorkers   = 3
)

func IterateEntities(entity string, fn func(data map[string]any) error) error {
	e := db()
	if e == nil {
		return nil
	}

	return IterateSnapshot(e.Snapshot(entity), fn)
}

func IterateSnapshot(snap *datastore.Snapshot, fn func(data map[string]any) error) error {
	var err error
	snap.Ascend(0, func(d datastore.Doc) bool {
		data, decodeErr := d.Decode()
		if decodeErr != nil {
			return true
		}

		err = fn(data)

		return err == nil
	})

	return err
}

func RestoreEntities(entity string, next func() (map[string]any, error)) (int, error) {
	AddEntityType(entity)

	count := 0
	batch := []map[string]any{}
	flush := func() error {
		if len(batch) == 0 {
			return nil
		}

		err := update(func(tx *datastore.Tx) error {
			for _, data := range batch {
				id, _ := data["id"].(string)
				if err := tx.Put(entity, id, data); err != nil {
					return err
				}
			}
			return nil
		})

		count += len(batch)
		batch = batch[:0]

		return err
	}

	for {
		data, err := next()
		if err == io.EOF {
			break
		}

		if err != nil {
			if flushErr := flush(); flushErr != nil {
				return count, flushErr
			}

			return count, err
		}

		batch = append(batch, data)
		if len(batch) >= restoreBatch {
			if err := flush(); err != nil {
				return count, err
			}
		}
	}

	return count, flush()
}

type rawDocument struct {
	id    string
	raw   []byte
	value []byte
	doc   map[string]any
}

func RestoreRawEntities(entity string, next func() (string, []byte, error)) (int, error) {
	AddEntityType(entity)

	type work struct {
		docs []rawDocument
		done chan error
	}

	queue := make(chan *work, rawBatchWorkers)
	var readErr error
	go func() {
		defer close(queue)

		slots := make(chan struct{}, rawBatchWorkers)
		batch := make([]rawDocument, 0, rawBatchDocuments)
		size := 0
		send := func() {
			if len(batch) == 0 {
				return
			}

			w := &work{docs: batch, done: make(chan error, 1)}
			slots <- struct{}{}
			queue <- w
			go func() {
				defer func() { <-slots }()
				for i := range w.docs {
					d := &w.docs[i]
					var err error
					if d.doc, err = datastore.DecodeMap(d.raw); err == nil {
						d.value, err = json.Marshal(d.doc)
					}
					if err != nil {
						w.done <- err
						return
					}
					d.raw = nil
				}
				w.done <- nil
			}()

			batch = make([]rawDocument, 0, rawBatchDocuments)
			size = 0
		}

		for {
			id, raw, err := next()
			if err != nil {
				if err != io.EOF {
					readErr = err
				}
				send()
				return
			}

			batch = append(batch, rawDocument{id: id, raw: raw})
			size += len(raw)
			if len(batch) >= rawBatchDocuments || size >= rawBatchBytes {
				send()
			}
		}
	}()

	count := 0
	var writeErr error
	for w := range queue {
		err := <-w.done
		if err == nil && writeErr == nil {
			err = update(func(tx *datastore.Tx) error {
				for i := range w.docs {
					d := &w.docs[i]
					if err := tx.PutBytes(entity, d.id, d.value, d.doc); err != nil {
						return err
					}
				}
				return nil
			})
			if err == nil {
				count += len(w.docs)
			}
		}
		if err != nil && writeErr == nil {
			writeErr = err
		}
	}

	if writeErr != nil {
		return count, writeErr
	}

	return count, readErr
}
