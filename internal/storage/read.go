package storage

import (
	"encoding/json"
	"io"
	"os"
	"strconv"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
)

func ReadFromDB(fileName string) (map[string][]byte, error) {
	cfg := globals.GetConfig()

	file, err := os.Open(cfg.Store.Folder + "/" + fileName)
	if err != nil {
		return nil, err
	}

	defer file.Close()
	log.Info("Successfully Opened "+fileName, " for reading.")

	data := make(map[string][]byte)

	byteValue, _ := io.ReadAll(file)

	if len(byteValue) == 0 {
		return data, nil
	}

	if err := json.Unmarshal(byteValue, &data); err != nil {
		return nil, err
	}

	return data, nil
}

func ReadExpirationsFromDB(fileName string) (map[int64][]string, error) {
	cfg := globals.GetConfig()

	f, err := os.Open(cfg.Store.Folder + "/" + fileName)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	bytes, err := io.ReadAll(f)
	if err != nil {
		return nil, err
	}
	if len(bytes) == 0 {
		return make(map[int64][]string), nil
	}

	var raw map[string][]string
	if err := json.Unmarshal(bytes, &raw); err != nil {
		return nil, err
	}

	out := make(map[int64][]string, len(raw))
	for k, list := range raw {
		ts, err := strconv.ParseInt(k, 10, 64)
		if err != nil {
			continue
		}
		cp := append([]string(nil), list...)
		out[ts] = cp
	}

	return out, nil
}

func ReadJsonFromDB(fileName string) (map[string]map[string]interface{}, error) {
	cfg := globals.GetConfig()

	file, err := os.Open(cfg.Store.Folder + "/" + fileName)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	data := make(map[string]map[string]interface{})
	bytes, err := io.ReadAll(file)
	if err != nil {
		return nil, err
	}
	if len(bytes) == 0 {
		return data, nil
	}

	if err := json.Unmarshal(bytes, &data); err != nil {
		return nil, err
	}

	return data, nil
}
