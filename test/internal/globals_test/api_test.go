package globals_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/globals"
)

func TestApiEntityKey(t *testing.T) {
	tests := []struct {
		entity string
		want   string
	}{
		{"users", "api:entity:users"},
		{"posts", "api:entity:posts"},
		{"weird-entity_name42", "api:entity:weird-entity_name42"},
		{"", "api:entity:"},
		{"équipe", "api:entity:équipe"},
	}
	for _, tt := range tests {
		if got := globals.ApiEntityKey(tt.entity); got != tt.want {
			t.Errorf("ApiEntityKey(%q) = %q, want %q", tt.entity, got, tt.want)
		}
	}
}

func TestApiEntitiesAllKey(t *testing.T) {
	tests := []struct {
		entity string
		want   string
	}{
		{"users", "api:entity:users:*"},
		{"orders", "api:entity:orders:*"},
		{"", "api:entity::*"},
		{"νέα", "api:entity:νέα:*"},
	}
	for _, tt := range tests {
		if got := globals.ApiEntitiesAllKey(tt.entity); got != tt.want {
			t.Errorf("ApiEntitiesAllKey(%q) = %q, want %q", tt.entity, got, tt.want)
		}
	}
}

func TestApiSingleEntityKey(t *testing.T) {
	tests := []struct {
		entity string
		id     string
		want   string
	}{
		{"users", "123", "api:entity:users:id:123"},
		{"articles", "a1b2c3", "api:entity:articles:id:a1b2c3"},
		{"", "", "api:entity::id:"},
		{"produits", "é-Ü-42", "api:entity:produits:id:é-Ü-42"},
	}
	for _, tt := range tests {
		if got := globals.ApiSingleEntityKey(tt.entity, tt.id); got != tt.want {
			t.Errorf("ApiSingleEntityKey(%q,%q) = %q, want %q", tt.entity, tt.id, got, tt.want)
		}
	}
}
