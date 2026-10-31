package security

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

type Session struct {
	ID        string `json:"id"`
	Username  string `json:"username"`
	Role      Role   `json:"role"`
	ExpiresAt int64  `json:"expires_at"`
}

type SessionsFile struct {
	Sessions []Session `json:"sessions"`
}

func GetCurrentUsername(ctx *fasthttp.RequestCtx) string {
	username, _ := ctx.UserValue("username").(string)

	return username
}

func CurrentUserCanManageUser(ctx *fasthttp.RequestCtx, username string) (bool, error) {
	currentSession, err := CurrentSession(ctx)
	if err != nil {
		return false, err
	}

	return CurrentUserIsAdmin(ctx) || currentSession.Username != username, nil
}

func CurrentSession(ctx *fasthttp.RequestCtx) (*Session, error) {
	currentSession, err := GetSession(string(ctx.Request.Header.Cookie(SessionCookieName)))
	if err != nil {
		return nil, err
	}

	return currentSession, nil
}

func CurrentUserIsAdmin(ctx *fasthttp.RequestCtx) bool {
	if !AuthenticationIsEnabled() {
		return true
	}

	if TokenAuthenticationIsEnabled() {
		return CheckTokenAuthentication(ctx)
	}

	if BasicAuthenticationIsEnabled() {
		return CheckBasicAuthentication(ctx)
	}

	currentSession, err := CurrentSession(ctx)
	if err != nil || currentSession == nil {
		return false
	}

	return currentSession.Role == RoleAdmin
}

func generateSessionID() (string, error) {
	b := make([]byte, 32)
	_, err := rand.Read(b)
	if err != nil {
		return "", err
	}

	return hex.EncodeToString(b), nil
}

func loadSessions() (*SessionsFile, error) {
	cfg := globals.GetConfig()
	path := fmt.Sprintf("%s/%s", cfg.Store.Folder, SessionsFilename)

	file, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return &SessionsFile{Sessions: []Session{}}, nil
		}

		return nil, err
	}

	defer file.Close()

	var sf SessionsFile
	dec := json.NewDecoder(file)
	if err := dec.Decode(&sf); err != nil {
		return nil, err
	}

	return &sf, nil
}

var sessionsMu sync.Mutex

func saveSessions(sf *SessionsFile) error {
	cfg := globals.GetConfig()
	path := fmt.Sprintf("%s/%s", cfg.Store.Folder, SessionsFilename)
	tmpPath := path + ".tmp"

	file, err := os.OpenFile(tmpPath, os.O_RDWR|os.O_CREATE|os.O_TRUNC, 0o644)
	if err != nil {
		return err
	}

	if err := json.NewEncoder(file).Encode(sf); err != nil {
		file.Close()
		os.Remove(tmpPath)

		return err
	}

	if err := file.Close(); err != nil {
		os.Remove(tmpPath)

		return err
	}

	return os.Rename(tmpPath, path)
}

func CreateSession(username string, role Role, ttl time.Duration) (*Session, error) {
	sessionsMu.Lock()
	defer sessionsMu.Unlock()

	sf, err := loadSessions()
	if err != nil {
		return nil, err
	}

	now := time.Now().Unix()
	var active []Session
	for _, s := range sf.Sessions {
		if s.ExpiresAt > now {
			active = append(active, s)
		}
	}

	sf.Sessions = active

	id, err := generateSessionID()
	if err != nil {
		return nil, err
	}

	session := Session{
		ID:        id,
		Username:  username,
		Role:      role,
		ExpiresAt: now + int64(ttl.Seconds()),
	}

	sf.Sessions = append(sf.Sessions, session)

	if err := saveSessions(sf); err != nil {
		return nil, err
	}

	return &session, nil
}

func GetSession(id string) (*Session, error) {
	sf, err := loadSessions()
	if err != nil {
		return nil, err
	}

	now := time.Now().Unix()
	var found *Session
	hasExpired := false

	for _, s := range sf.Sessions {
		if s.ExpiresAt <= now {
			hasExpired = true
			continue
		}

		if s.ID == id {
			tmp := s
			found = &tmp
		}
	}

	if hasExpired {
		_ = removeExpiredSessions()
	}

	if found == nil {
		return nil, nil
	}

	return found, nil
}

func removeExpiredSessions() error {
	sessionsMu.Lock()
	defer sessionsMu.Unlock()

	sf, err := loadSessions()
	if err != nil {
		return err
	}

	now := time.Now().Unix()
	active := make([]Session, 0, len(sf.Sessions))
	for _, s := range sf.Sessions {
		if s.ExpiresAt > now {
			active = append(active, s)
		}
	}

	if len(active) == len(sf.Sessions) {
		return nil
	}

	sf.Sessions = active

	return saveSessions(sf)
}

func DeleteSession(id string) error {
	sessionsMu.Lock()
	defer sessionsMu.Unlock()

	sf, err := loadSessions()
	if err != nil {
		return err
	}

	var updated []Session
	for _, s := range sf.Sessions {
		if s.ID != id {
			updated = append(updated, s)
		}
	}

	sf.Sessions = updated

	return saveSessions(sf)
}
