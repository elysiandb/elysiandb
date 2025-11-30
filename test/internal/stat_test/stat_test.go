package stat_test

import (
	"encoding/json"
	"sync"
	"testing"

	"github.com/taymour/elysiandb/internal/stat"
)

func TestNewStatsContainer_Zeroed(t *testing.T) {
	s := stat.NewStatsContainer()

	var m map[string]string
	if err := json.Unmarshal([]byte(s.ToJson()), &m); err != nil {
		t.Fatalf("json.Unmarshal failed: %v", err)
	}

	wantZero := map[string]string{
		"keys_count":            "0",
		"expiration_keys_count": "0",
		"uptime_seconds":        "0",
		"total_requests":        "0",
		"hits":                  "0",
		"misses":                "0",
		"version":               "0.1.9",
	}
	for k, want := range wantZero {
		if got := m[k]; got != want {
			t.Errorf("field %q = %q, want %q", k, got, want)
		}
	}
}

func TestIncrementsAndSets(t *testing.T) {
	s := stat.NewStatsContainer()

	s.IncrementKeysCount()
	s.IncrementExpirationKeysCount()
	s.IncrementUptimeSeconds()
	s.IncrementTotalRequests()
	s.IncrementHits()
	s.IncrementMisses()

	s.SetKeysCount(42)
	s.SetExpirationKeysCount(7)

	var m map[string]string
	if err := json.Unmarshal([]byte(s.ToJson()), &m); err != nil {
		t.Fatalf("json.Unmarshal failed: %v", err)
	}

	tests := map[string]string{
		"keys_count":            "42",
		"expiration_keys_count": "7",
		"uptime_seconds":        "1",
		"total_requests":        "1",
		"hits":                  "1",
		"misses":                "1",
		"version":               "0.1.9",
	}
	for k, want := range tests {
		if got := m[k]; got != want {
			t.Errorf("field %q = %q, want %q", k, got, want)
		}
	}
}

func TestDecrementDoesNotGoBelowZero(t *testing.T) {
	s := stat.NewStatsContainer()

	s.DecrementKeysCount()
	s.DecrementExpirationKeysCount()

	s.IncrementKeysCount()
	s.IncrementExpirationKeysCount()
	s.DecrementKeysCount()
	s.DecrementExpirationKeysCount()

	var m map[string]string
	if err := json.Unmarshal([]byte(s.ToJson()), &m); err != nil {
		t.Fatalf("json.Unmarshal failed: %v", err)
	}
	if m["keys_count"] != "0" {
		t.Errorf("keys_count = %s, want 0", m["keys_count"])
	}
	if m["expiration_keys_count"] != "0" {
		t.Errorf("expiration_keys_count = %s, want 0", m["expiration_keys_count"])
	}
}

func TestReset(t *testing.T) {
	s := stat.NewStatsContainer()

	s.SetKeysCount(10)
	s.SetExpirationKeysCount(5)
	s.IncrementUptimeSeconds()
	s.IncrementTotalRequests()
	s.IncrementHits()
	s.IncrementMisses()

	s.Reset()

	var m map[string]string
	if err := json.Unmarshal([]byte(s.ToJson()), &m); err != nil {
		t.Fatalf("json.Unmarshal failed: %v", err)
	}
	for k, v := range m {
		if k == "version" {
			continue
		}

		if v != "0" {
			t.Errorf("after Reset, %s = %s, want 0", k, v)
		}
	}
}

func TestToJSON_ValuesAreStrings(t *testing.T) {
	s := stat.NewStatsContainer()
	s.SetKeysCount(3)
	s.SetExpirationKeysCount(4)
	s.IncrementUptimeSeconds()
	s.IncrementTotalRequests()
	s.IncrementHits()
	s.IncrementMisses()

	var m map[string]interface{}
	if err := json.Unmarshal([]byte(s.ToJson()), &m); err != nil {
		t.Fatalf("json.Unmarshal failed: %v", err)
	}
	for k, v := range m {
		if _, ok := v.(string); !ok {
			t.Errorf("field %q is not a string in JSON (got %T: %v)", k, v, v)
		}
	}
}

func TestConcurrentIncrements_NoRaceAndCountsMatch(t *testing.T) {
	s := stat.NewStatsContainer()

	const goroutines = 64
	const iters = 10_000

	var wg sync.WaitGroup
	wg.Add(goroutines)

	for i := 0; i < goroutines; i++ {
		go func() {
			defer wg.Done()
			for j := 0; j < iters; j++ {
				s.IncrementKeysCount()
				s.IncrementExpirationKeysCount()
				s.IncrementTotalRequests()
				s.IncrementHits()
			}
		}()
	}

	wg.Wait()

	var m map[string]string
	if err := json.Unmarshal([]byte(s.ToJson()), &m); err != nil {
		t.Fatalf("json.Unmarshal failed: %v", err)
	}

	want := goroutines * iters
	check := func(field string) {
		if m[field] != itoa(want) {
			t.Errorf("%s = %s, want %d", field, m[field], want)
		}
	}
	check("keys_count")
	check("expiration_keys_count")
	check("total_requests")
	check("hits")
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	sign := ""
	if n < 0 {
		sign = "-"
		n = -n
	}
	buf := [20]byte{}
	i := len(buf)
	for n > 0 {
		i--
		buf[i] = byte('0' + n%10)
		n /= 10
	}
	if sign != "" {
		i--
		buf[i] = '-'
	}
	return string(buf[i:])
}

func TestInit_GlobalStatsIsZeroed(t *testing.T) {
	stat.Stats.IncrementHits()
	stat.Stats.IncrementMisses()
	stat.Stats.SetKeysCount(9)
	stat.Stats.SetExpirationKeysCount(2)

	stat.Init()

	var m map[string]string
	if err := json.Unmarshal([]byte(stat.Stats.ToJson()), &m); err != nil {
		t.Fatalf("json.Unmarshal failed: %v", err)
	}
	for k, v := range m {
		if k == "version" {
			continue
		}

		if v != "0" {
			t.Errorf("after Init, %s = %s, want 0", k, v)
		}
	}
}
