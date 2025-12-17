package storage

import (
	"encoding/json"
	"os"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/recovery"
)

func WriteToDB() {
	WriteStoreDB()
	WriteJsonDB()
}

func WriteStoreDB() {
	cfg := globals.GetConfig()

	rootMu.RLock()
	ms := mainStore
	ec := expirationContainer
	rootMu.RUnlock()

	if err := writeStoreToFile(cfg, DataFile, ms); err != nil {
		log.Error("Error writing main store to database:", err)

		return
	}

	if err := writeExpirationsToFile(cfg, ExpirationDataFile, ec); err != nil {
		log.Error("Error writing expiration store to database:", err)

		return
	}

	if cfg.Store.CrashRecovery.Enabled {
		recovery.ClearStoreRecoveryLog()
	}
}

func WriteJsonDB() {
	cfg := globals.GetConfig()

	rootMu.RLock()
	js := mainJsonStore.Load()
	rootMu.RUnlock()

	if js == nil {
		return
	}

	if err := writeJsonStoreToFile(cfg, JsonDataFile, js); err != nil {
		log.Error("Error writing json store to database:", err)

		return
	}

	if cfg.Store.CrashRecovery.Enabled {
		recovery.ClearJsonRecoveryLog()
	}
}

func writeExpirationsToFile(cfg *configuration.Config, fileName string, expirationContainer *ExpirationContainer) error {
	if expirationContainer.saved.Load() {
		return nil
	}

	isSuccess := true

	path := cfg.Store.Folder + "/" + fileName

	file, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o644)
	if err != nil {
		isSuccess = false
		log.Error("Error opening file:", err)
	}
	defer file.Close()

	encoder := json.NewEncoder(file)

	expirationsAsMap := expirationContainer.ToMap()

	if err := encoder.Encode(expirationsAsMap); err != nil {
		isSuccess = false
		log.Error("Error encoding JSON:", err)
	}

	expirationContainer.saved.Store(isSuccess)

	return nil
}

func writeStoreToFile(cfg *configuration.Config, fileName string, store *Store) error {
	if store.saved.Load() {
		return nil
	}

	isSuccess := true

	path := cfg.Store.Folder + "/" + fileName

	file, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o644)
	if err != nil {
		isSuccess = false
		log.Error("Error opening file:", err)
	}
	defer file.Close()

	encoder := json.NewEncoder(file)

	storeAsMap := store.ToMap()

	if err := encoder.Encode(storeAsMap); err != nil {
		isSuccess = false
		log.Error("Error encoding JSON:", err)
	}

	store.saved.Store(isSuccess)

	return nil
}
