package api_test

import (
	"encoding/json"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
)

func TestParseMigrationQuery_SingleSet(t *testing.T) {
	body := `[{"set": [{"name": "test"}]}]`
	result := api_storage.ParseMigrationQuery(body, "users")

	if len(result) != 1 {
		t.Fatalf("expected 1 query, got %d", len(result))
	}

	q := result[0]
	if q.Entity != "users" {
		t.Errorf("expected entity 'users', got %s", q.Entity)
	}
	if q.Action != "set" {
		t.Errorf("expected action 'set', got %s", q.Action)
	}
	if q.Properties["name"] != "test" {
		t.Errorf("expected property name 'test', got %v", q.Properties["name"])
	}
}

func TestParseMigrationQuery_InvalidJSON(t *testing.T) {
	body := `invalid json`
	result := api_storage.ParseMigrationQuery(body, "users")
	if result != nil {
		t.Errorf("expected nil for invalid JSON")
	}
}

func TestExecuteMigrations_UnsupportedAction(t *testing.T) {
	q := []api_storage.MigrationQuery{
		{Entity: "users", Action: "unknown", Properties: map[string]any{"name": "x"}},
	}
	err := api_storage.ExecuteMigrations(q)
	if err == nil {
		t.Errorf("expected error for unsupported action")
	}
}

func TestParseMigrationQuery_ObjectSyntax(t *testing.T) {
	obj := map[string]any{"set": map[string]any{"age": 30}}
	raw, _ := json.Marshal([]any{obj})
	result := api_storage.ParseMigrationQuery(string(raw), "users")

	if len(result) != 1 {
		t.Fatalf("expected 1 query, got %d", len(result))
	}
	if result[0].Properties["age"] != float64(30) {
		t.Errorf("expected property age 30, got %v", result[0].Properties["age"])
	}
}
