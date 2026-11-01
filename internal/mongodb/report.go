package mongodb

import (
	"time"

	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
)

var (
	mongoLog      = log.For("mongodb")
	errorThrottle = log.NewThrottle(time.Minute)
)

func reportError(op, collection string, err error) {
	metrics.MongoError()

	if suppressed, ok := errorThrottle.Allow(op + "|" + collection); ok {
		mongoLog.Error("mongodb operation failed", log.WithSuppressed([]any{"op", op, "collection", collection, "err", err}, suppressed)...)
	}
}
