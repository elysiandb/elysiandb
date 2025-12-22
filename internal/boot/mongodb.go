package boot

import (
	"context"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

func InitMongoDBConnection() {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	client, err := mongo.Connect(options.Client().ApplyURI(globals.GetConfig().Engine.URI))
	if err != nil {
		panic(err)
	}

	if err := client.Ping(ctx, nil); err != nil {
		panic(err)
	}

	globals.MongoClient = client
	globals.MongoDB = client.Database("elysiandb")
}
