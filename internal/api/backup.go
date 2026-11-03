package api_storage

import (
	"io"

	"github.com/taymour/elysiandb/internal/datastore"
)

const restoreBatch = 1000

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
