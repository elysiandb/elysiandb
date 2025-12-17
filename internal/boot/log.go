package boot

import (
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
)

func BootLogger() {
	d := time.Duration(globals.GetConfig().Log.FlushIntervalSeconds) * time.Second
	if d <= 0 {
		return
	}

	go WriteLogsPeriodically(d)
}

func WriteLogsPeriodically(interval time.Duration) {
	for {
		log.WriteLogs()
		time.Sleep(interval)
	}
}
