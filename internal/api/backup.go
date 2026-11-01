package api_storage

import (
	"io"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
)

func IterateEntities(entity string, fn func(data map[string]any) error) error {
	idList, _ := GetListOfIds(entity, "", true)
	for _, id := range decodeIDs(idList) {
		data := ReadEntityById(entity, id)
		if data == nil {
			continue
		}

		if err := fn(data); err != nil {
			return err
		}
	}

	return nil
}

func RestoreEntities(entity string, next func() (map[string]any, error)) (int, error) {
	AddEntityType(entity)

	ids := make([]string, 0)
	for {
		data, err := next()
		if err == io.EOF {
			break
		}

		if err != nil {
			return len(ids), err
		}

		id, _ := data["id"].(string)
		storage.PutJsonValue(globals.ApiSingleEntityKey(entity, id), data)
		ids = append(ids, id)
	}

	if len(ids) > 0 {
		storage.PutKeyValue(globals.ApiEntityIndexIdKey(entity), encodeIDs(ids))
	}

	return len(ids), nil
}
