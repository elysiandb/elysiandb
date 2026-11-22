package handler

import (
	"github.com/taymour/elysiandb/internal/storage"
)

func HandleSave() []byte {

	storage.WriteToDB()
	return []byte("OK")
}
