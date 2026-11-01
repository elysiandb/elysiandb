package mongodb_test

import (
	"context"
	"fmt"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/query"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/storage"
	"go.mongodb.org/mongo-driver/v2/bson"
)

func connectMongoDB(t *testing.T) string {
	t.Helper()

	uri := os.Getenv("ELYSIAN_TEST_MONGODB_URI")
	if uri == "" {
		t.Skip("ELYSIAN_TEST_MONGODB_URI is not set")
	}

	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Engine.Name = "mongodb"
	cfg.Engine.URI = uri
	cfg.Api.Schema.Enabled = true
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()
	boot.InitMongoDBConnection()

	entity := fmt.Sprintf("it_%d", time.Now().UnixNano())
	t.Cleanup(func() {
		ctx := context.Background()
		_ = globals.MongoDB.Collection(entity).Drop(ctx)
		_, _ = globals.MongoDB.Collection(schema.SchemaEntity).DeleteOne(ctx, bson.M{"_id": entity})
		_ = globals.MongoClient.Disconnect(ctx)
	})

	return entity
}

func TestMongoDBExecuteQueryUsesTheFirstSortKeyInRequestOrder(t *testing.T) {
	entity := connectMongoDB(t)

	for _, doc := range []map[string]any{
		{"id": "f1", "title": "C", "year": 1983.0},
		{"id": "f2", "title": "B", "year": 1982.0},
		{"id": "f3", "title": "A", "year": 1979.0},
	} {
		if errs := mongodb.WriteEntity(entity, doc); len(errs) > 0 {
			t.Fatalf("write: %v", errs)
		}
	}

	ids := func(sortOrder ...string) []string {
		out, err := mongodb.ExecuteQuery(query.Query{
			Entity:    entity,
			Filter:    query.FilterNode{Leaf: map[string]map[string]string{}},
			Sorts:     map[string]string{"year": "desc", "title": "asc"},
			SortOrder: sortOrder,
		})
		if err != nil {
			t.Fatal(err)
		}

		var got []string
		for _, item := range out {
			got = append(got, item["id"].(string))
		}

		return got
	}

	if got := ids("year", "title"); !reflect.DeepEqual(got, []string{"f1", "f2", "f3"}) {
		t.Fatalf("expected year desc first, got %v", got)
	}

	if got := ids("title", "year"); !reflect.DeepEqual(got, []string{"f3", "f2", "f1"}) {
		t.Fatalf("expected title asc first, got %v", got)
	}
}

func TestMongoDBInferredSchemaIsExtended(t *testing.T) {
	entity := connectMongoDB(t)

	if errs := mongodb.WriteEntity(entity, map[string]any{"id": "i1", "a": "x"}); len(errs) > 0 {
		t.Fatalf("write: %v", errs)
	}
	if errs := mongodb.WriteEntity(entity, map[string]any{"id": "i2", "b": 1.0}); len(errs) > 0 {
		t.Fatalf("write: %v", errs)
	}

	fields, _ := mongodb.GetEntitySchema(entity)["fields"].(map[string]any)
	for name, typ := range map[string]string{"a": "string", "b": "number"} {
		field, ok := fields[name].(map[string]any)
		if !ok || field["type"] != typ {
			t.Fatalf("expected %s of type %s in the schema, got %v", name, typ, fields)
		}
	}

	if errs := mongodb.WriteEntity(entity, map[string]any{"id": "i3", "a": 5.0}); len(errs) == 0 {
		t.Fatalf("expected a type change on an existing field to be rejected")
	}
}

func TestMongoDBIncludesDoNotResolveCoreEntities(t *testing.T) {
	entity := connectMongoDB(t)
	t.Cleanup(func() {
		_, _ = globals.MongoDB.Collection("_elysiandb_core_user").DeleteOne(context.Background(), bson.M{"_id": entity + "-admin"})
	})

	if errs := mongodb.WriteEntity("_elysiandb_core_user", map[string]any{"id": entity + "-admin", "username": "admin", "password": "hash", "role": "admin"}); len(errs) > 0 {
		t.Fatalf("write user: %v", errs)
	}

	link := map[string]any{"@entity": "_elysiandb_core_user", "id": entity + "-admin"}
	if errs := mongodb.WriteEntity(entity, map[string]any{"id": "l1", "u": link}); len(errs) > 0 {
		t.Fatalf("write: %v", errs)
	}

	for _, includes := range []string{"u", "all", "_elysiandb_core_users"} {
		out := mongodb.ListEntities(entity, 0, 0, "", true, nil, "", includes)
		raw := fmt.Sprintf("%v", out)
		if len(out) != 1 || strings.Contains(raw, "hash") || strings.Contains(raw, "role") {
			t.Fatalf("includes=%s resolved a core entity: %v", includes, out)
		}
	}
}

func TestMongoDBCountsDocumentsPerEntityTypeAndPings(t *testing.T) {
	entity := connectMongoDB(t)

	for _, id := range []string{"c1", "c2", "c3"} {
		if errs := mongodb.WriteEntity(entity, map[string]any{"id": id, "n": 1.0}); len(errs) > 0 {
			t.Fatalf("write: %v", errs)
		}
	}

	if got := mongodb.CountEntitiesPerType()[entity]; got != 3 {
		t.Fatalf("expected 3 documents for %s, got %d", entity, got)
	}

	if d, err := boot.MongoPing(); err != nil || d <= 0 {
		t.Fatalf("expected a successful ping, got %v %v", d, err)
	}

	if host := boot.MongoHost(os.Getenv("ELYSIAN_TEST_MONGODB_URI")); host == "unknown" || strings.Contains(host, "@") {
		t.Fatalf("unexpected host %q", host)
	}
}
