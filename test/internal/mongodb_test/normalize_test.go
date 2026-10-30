package mongodb_test

import (
	"reflect"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/mongodb"
	"go.mongodb.org/mongo-driver/v2/bson"
)

func TestToMongoDocumentKeepsValuesAsIs(t *testing.T) {
	data := map[string]any{
		"id":      "a1",
		"_id":     "user value",
		"created": "2024-01-15T10:30:00.123+02:00",
		"nested":  map[string]any{"at": "2024-01-15T10:30:00Z"},
		"tags":    []any{"x", float64(1)},
	}

	doc := mongodb.ToMongoDocument(data)

	want := bson.M{
		"_id":                     "a1",
		mongodb.StoredUserIdField: "user value",
		"created":                 "2024-01-15T10:30:00.123+02:00",
		"nested":                  map[string]any{"at": "2024-01-15T10:30:00Z"},
		"tags":                    []any{"x", float64(1)},
	}
	if !reflect.DeepEqual(doc, want) {
		t.Fatalf("unexpected document: %#v", doc)
	}
}

func TestToMongoDocumentWithoutStringIdLetsMongoGenerateOne(t *testing.T) {
	doc := mongodb.ToMongoDocument(map[string]any{"id": float64(3), "name": "x"})
	if _, ok := doc["_id"]; ok {
		t.Fatalf("expected no _id, got %#v", doc)
	}
}

func TestFromMongoDocumentRestoresId(t *testing.T) {
	oid := bson.NewObjectID()

	if got := mongodb.FromMongoDocument(map[string]any{"_id": "a1", "x": "y"}); !reflect.DeepEqual(got, map[string]any{"id": "a1", "x": "y"}) {
		t.Fatalf("unexpected document: %#v", got)
	}

	stored := map[string]any{"_id": "a1", mongodb.StoredUserIdField: int32(7)}
	if got := mongodb.FromMongoDocument(stored); !reflect.DeepEqual(got, map[string]any{"id": "a1", "_id": float64(7)}) {
		t.Fatalf("expected the user _id field to be restored, got %#v", got)
	}

	if got := mongodb.FromMongoDocument(map[string]any{"_id": oid}); got["id"] != oid.Hex() {
		t.Fatalf("expected ObjectID hex, got %#v", got)
	}
}

func TestNormalizeMongoValueConvertsDriverTypes(t *testing.T) {
	when := time.Date(2024, 1, 15, 8, 30, 0, 123000000, time.UTC)

	got := mongodb.NormalizeMongoValue(bson.D{
		{Key: "legacy", Value: bson.NewDateTimeFromTime(when)},
		{Key: "time", Value: when.In(time.FixedZone("x", 7200))},
		{Key: "list", Value: bson.A{bson.D{{Key: "_id", Value: "kept"}}, bson.M{"k": "v"}}},
		{Key: "plain", Value: map[string]any{"n": float64(1)}},
		{Key: "int32", Value: int32(10)},
		{Key: "int64", Value: int64(-3)},
	})

	want := map[string]any{
		"legacy": "2024-01-15T08:30:00.123Z",
		"time":   "2024-01-15T08:30:00.123Z",
		"list":   []any{map[string]any{"_id": "kept"}, map[string]any{"k": "v"}},
		"plain":  map[string]any{"n": float64(1)},
		"int32":  float64(10),
		"int64":  float64(-3),
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("unexpected value: %#v", got)
	}
}

func TestNormalizeMongoValueLegacyDateWithoutFraction(t *testing.T) {
	got := mongodb.NormalizeMongoValue(bson.NewDateTimeFromTime(time.Date(2024, 1, 15, 10, 0, 0, 0, time.UTC)))
	if got != "2024-01-15T10:00:00Z" {
		t.Fatalf("unexpected value: %#v", got)
	}
}
