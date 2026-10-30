package mongotest

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"os"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

const URIEnv = "ELYSIANDB_TEST_MONGODB_URI"

func URI(t testing.TB) string {
	t.Helper()

	uri := os.Getenv(URIEnv)
	if uri != "" {
		return uri
	}

	if os.Getenv("CI") != "" {
		t.Fatalf("%s must be set in CI so that the MongoDB engine is tested", URIEnv)
	}

	t.Skipf("%s is not set: start a MongoDB server and export it to run the MongoDB engine tests", URIEnv)

	return ""
}

func DatabaseName(t testing.TB) string {
	t.Helper()

	b := make([]byte, 6)
	if _, err := rand.Read(b); err != nil {
		t.Fatalf("random database name: %v", err)
	}

	return "elysiandb_test_" + hex.EncodeToString(b)
}

func Connect(t testing.TB) *mongo.Database {
	t.Helper()

	uri := URI(t)

	client, err := mongo.Connect(options.Client().ApplyURI(uri))
	if err != nil {
		t.Fatalf("connect to MongoDB: %v", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := client.Ping(ctx, nil); err != nil {
		t.Fatalf("ping MongoDB at %s: %v", uri, err)
	}

	db := client.Database(DatabaseName(t))
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()

		_ = db.Drop(ctx)
		_ = client.Disconnect(ctx)
	})

	return db
}

func Use(t testing.TB) *mongo.Database {
	t.Helper()

	db := Connect(t)
	previousClient, previousDB := globals.MongoClient, globals.MongoDB
	globals.MongoClient, globals.MongoDB = db.Client(), db
	t.Cleanup(func() {
		globals.MongoClient, globals.MongoDB = previousClient, previousDB
	})

	return db
}
