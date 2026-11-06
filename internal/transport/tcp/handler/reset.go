package handler

import (
	"github.com/taymour/elysiandb/internal/storage"
)

func HandleReset() []byte {

	storage.ResetStore()
	return []byte("OK")
}
