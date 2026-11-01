package mongodb_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/mongodb"
)

func TestMongoDBImportReplacesTheEntityAndChecksManualSchemasOnly(t *testing.T) {
	entity := connectMongoDB(t)

	mongodb.WriteEntity(entity, map[string]any{"id": "f1", "n": 1.0})
	mongodb.WriteEntity(entity, map[string]any{"id": "f2", "n": 2.0})

	if rejected := mongodb.ImportAll(map[string][]map[string]any{entity: {{"id": "f1", "n": "ten"}}}); len(rejected) > 0 {
		t.Fatalf("an inferred schema must not reject imported documents, got %v", rejected)
	}

	if got := mongodb.ReadEntityById(entity, "f1"); got == nil || got["n"] != "ten" {
		t.Fatalf("expected the imported version of f1, got %v", got)
	}

	if mongodb.ReadEntityById(entity, "f2") != nil {
		t.Fatalf("documents that are not in the dump must be deleted")
	}

	mongodb.UpdateEntitySchema(entity, map[string]any{"title": map[string]any{"type": "string", "required": true}})
	if rejected := mongodb.ImportAll(map[string][]map[string]any{entity: {{"id": "ko", "title": 5.0}}}); len(rejected) != 1 || rejected[0].ID != "ko" {
		t.Fatalf("expected the manual schema to reject ko, got %+v", rejected)
	}
}
