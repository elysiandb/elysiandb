package api_test

import (
	"reflect"
	"testing"

	api_storage "github.com/taymour/elysiandb/internal/api"
)

func TestFieldGetNestedValue(t *testing.T) {
	data := map[string]interface{}{
		"name": "John",
		"address": map[string]interface{}{
			"city":  "Paris",
			"geo":   map[string]interface{}{"lat": 48.85, "lng": 2.35},
			"empty": map[string]interface{}{},
		},
	}

	tests := []struct {
		path     string
		expected interface{}
		found    bool
	}{
		{"name", "John", true},
		{"address.city", "Paris", true},
		{"address.geo.lat", 48.85, true},
		{"address.geo.lng", 2.35, true},
		{"address.geo.missing", nil, false},
		{"missing", nil, false},
		{"address.empty", map[string]interface{}{}, true},
	}

	for _, tt := range tests {
		got, ok := api_storage.GetNestedValue(data, tt.path)
		if ok != tt.found {
			t.Errorf("GetNestedValue(%q) found=%v, want %v", tt.path, ok, tt.found)
		}
		if ok && !reflect.DeepEqual(got, tt.expected) {
			t.Errorf("GetNestedValue(%q)=%v, want %v", tt.path, got, tt.expected)
		}
	}
}

func TestSetNestedField(t *testing.T) {
	dest := make(map[string]interface{})
	api_storage.FilterFields(map[string]interface{}{}, []string{}) // force package load
	api_storage.SetNestedField(dest, "a.b.c", 42)
	expected := map[string]interface{}{
		"a": map[string]interface{}{
			"b": map[string]interface{}{
				"c": 42,
			},
		},
	}
	if !reflect.DeepEqual(dest, expected) {
		t.Errorf("SetNestedField result=%v, want %v", dest, expected)
	}
}

func TestFilterFields(t *testing.T) {
	data := map[string]interface{}{
		"id":   "u1",
		"name": "Alice",
		"address": map[string]interface{}{
			"city": "Paris",
			"geo":  map[string]interface{}{"lat": 48.85, "lng": 2.35},
		},
	}
	fields := []string{"id", "address.geo.lat"}
	got := api_storage.FilterFields(data, fields)
	expected := map[string]interface{}{
		"id": "u1",
		"address": map[string]interface{}{
			"geo": map[string]interface{}{
				"lat": 48.85,
			},
		},
	}
	if !reflect.DeepEqual(got, expected) {
		t.Errorf("FilterFields=%v, want %v", got, expected)
	}
}

func TestParseFieldsParam(t *testing.T) {
	tests := []struct {
		input    string
		expected []string
	}{
		{"name,email", []string{"name", "email"}},
		{" name , address.city ", []string{"name", "address.city"}},
		{"", nil},
		{",,,", []string{}},
	}
	for _, tt := range tests {
		got := api_storage.ParseFieldsParam(tt.input)
		if !reflect.DeepEqual(got, tt.expected) {
			t.Errorf("ParseFieldsParam(%q)=%v, want %v", tt.input, got, tt.expected)
		}
	}
}
