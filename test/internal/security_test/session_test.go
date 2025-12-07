package security_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
)

func setupTempStore(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	cfg := globals.GetConfig()
	cfg.Store.Folder = dir
	globals.SetConfig(cfg)
	return dir
}

func writeSessions(t *testing.T, dir string, sf security.SessionsFile) {
	t.Helper()
	b, _ := json.Marshal(sf)
	_ = os.WriteFile(filepath.Join(dir, security.SessionsFilename), b, 0644)
}

func TestCreateSession(t *testing.T) {
	dir := setupTempStore(t)

	s, err := security.CreateSession("user1", security.RoleAdmin, time.Second)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if s.Username != "user1" || s.Role != security.RoleAdmin {
		t.Fatalf("invalid session data")
	}
	if s.ID == "" {
		t.Fatalf("session id not generated")
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 1 {
		t.Fatalf("session not saved")
	}
}

func TestGetSession(t *testing.T) {
	dir := setupTempStore(t)

	s := security.Session{
		ID:        "abc",
		Username:  "user",
		Role:      security.RoleUser,
		ExpiresAt: time.Now().Unix() + 1000,
	}
	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{s}})

	res, err := security.GetSession("abc")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res == nil || res.Username != "user" {
		t.Fatalf("session not returned")
	}
}

func TestGetSessionExpired(t *testing.T) {
	dir := setupTempStore(t)

	s := security.Session{
		ID:        "expired",
		Username:  "user",
		Role:      security.RoleUser,
		ExpiresAt: time.Now().Unix() - 10,
	}
	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{s}})

	res, err := security.GetSession("expired")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res != nil {
		t.Fatalf("expired session should be nil")
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 0 {
		t.Fatalf("expired session not cleaned")
	}
}

func TestDeleteSession(t *testing.T) {
	dir := setupTempStore(t)

	s := security.Session{
		ID:        "to_delete",
		Username:  "user",
		Role:      security.RoleUser,
		ExpiresAt: time.Now().Unix() + 1000,
	}
	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{s}})

	err := security.DeleteSession("to_delete")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 0 {
		t.Fatalf("session not deleted")
	}
}

func TestDeleteSessionNonExisting(t *testing.T) {
	dir := setupTempStore(t)

	writeSessions(t, dir, security.SessionsFile{Sessions: []security.Session{}})

	err := security.DeleteSession("none")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(dir, security.SessionsFilename))
	var sf security.SessionsFile
	_ = json.Unmarshal(data, &sf)
	if len(sf.Sessions) != 0 {
		t.Fatalf("file modified incorrectly")
	}
}
