package boot

import (
	"github.com/taymour/elysiandb/internal/storage"
)

func InitDB() {
	storage.LoadDB()
	storage.LoadJsonDB()
	BootSaver()
	BootExpirationHandler()
	BootLogger()
}
