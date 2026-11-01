package boot

import (
	"context"
	"net/url"
	"sync/atomic"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"go.mongodb.org/mongo-driver/v2/event"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

var (
	mongoLog       = log.For("mongodb")
	mongoPoolInUse atomic.Int64
)

func InitMongoDBConnection() {
	start := time.Now()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	uri := globals.GetConfig().Engine.URI
	host := MongoHost(uri)

	client, err := mongo.Connect(options.Client().ApplyURI(uri).SetPoolMonitor(mongoPoolMonitor()))
	if err != nil {
		log.Fatal("mongodb connection failed", "component", "mongodb", "host", host, "err", err)
	}

	if err := client.Ping(ctx, nil); err != nil {
		log.Fatal("mongodb connection failed", "component", "mongodb", "host", host, "err", err)
	}

	globals.MongoClient = client
	globals.MongoDB = client.Database("elysiandb")

	mongoLog.Info("mongodb connected", "host", host, "database", "elysiandb", "duration_ms", log.Since(start))
}

func MongoHost(uri string) string {
	parsed, err := url.Parse(uri)
	if err != nil || parsed.Host == "" {
		return "unknown"
	}

	return parsed.Host
}

func mongoPoolMonitor() *event.PoolMonitor {
	return &event.PoolMonitor{
		Event: func(e *event.PoolEvent) {
			switch e.Type {
			case event.ConnectionCheckedOut:
				mongoPoolInUse.Add(1)
			case event.ConnectionCheckedIn:
				mongoPoolInUse.Add(-1)
			}
		},
	}
}

func MongoPing() (time.Duration, error) {
	client := globals.MongoClient
	if client == nil {
		return 0, mongo.ErrClientDisconnected
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	start := time.Now()
	err := client.Ping(ctx, nil)

	return time.Since(start), err
}
