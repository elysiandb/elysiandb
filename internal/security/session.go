package security

import (
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"sync"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

const SessionTTL = 24 * time.Hour

type Session struct {
	ID          string `json:"-"`
	TokenHash   string `json:"token_hash"`
	Username    string `json:"username"`
	Role        Role   `json:"role"`
	Credentials string `json:"credentials"`
	ExpiresAt   int64  `json:"expires_at"`
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

	if currentSession == nil {
		return false, nil
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

func hashSessionID(id string) string {
	sum := sha256.Sum256([]byte(id))

	return hex.EncodeToString(sum[:])
}

func credentialsFingerprint(passwordHash string) string {
	sum := sha256.Sum256([]byte(passwordHash))

	return hex.EncodeToString(sum[:16])
}

func storedUser(username string) (*BasicHashedUser, bool) {
	data := api_storage.ReadEntityById(UserEntity, username)
	if data == nil {
		return nil, false
	}

	user := &BasicHashedUser{}
	if err := user.FromDataMap(data); err != nil {
		return nil, false
	}

	return user, true
}

type sessionsCacheEntry struct {
	path   string
	file   SessionsFile
	byHash map[string]int
}

var (
	sessionsCacheMu sync.Mutex
	sessionsCache   *sessionsCacheEntry
)

func sessionsPath() string {
	cfg := globals.GetConfig()

	return fmt.Sprintf("%s/%s", cfg.Store.Folder, SessionsFilename)
}

func copySessions(sf SessionsFile) *SessionsFile {
	return &SessionsFile{Sessions: append([]Session(nil), sf.Sessions...)}
}

func newSessionsCacheEntry(path string, sf SessionsFile) *sessionsCacheEntry {
	entry := &sessionsCacheEntry{path: path, file: *copySessions(sf), byHash: make(map[string]int, len(sf.Sessions))}
	for i, s := range entry.file.Sessions {
		if s.TokenHash != "" {
			entry.byHash[s.TokenHash] = i
		}
	}

	return entry
}

func cachedSessions() (*sessionsCacheEntry, error) {
	path := sessionsPath()

	sessionsCacheMu.Lock()
	cached := sessionsCache
	sessionsCacheMu.Unlock()

	if cached != nil && cached.path == path {
		return cached, nil
	}

	sf, err := readSessionsFile(path)
	if err != nil {
		return nil, err
	}

	entry := newSessionsCacheEntry(path, *sf)
	cacheSessions(entry)

	return entry, nil
}

func readSessionsFile(path string) (*SessionsFile, error) {
	file, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return &SessionsFile{Sessions: []Session{}}, nil
		}

		return nil, err
	}

	defer file.Close()

	var sf SessionsFile
	if err := json.NewDecoder(file).Decode(&sf); err != nil {
		return nil, err
	}

	return &sf, nil
}

func loadSessions() (*SessionsFile, error) {
	entry, err := cachedSessions()
	if err != nil {
		return nil, err
	}

	return copySessions(entry.file), nil
}

func cacheSessions(entry *sessionsCacheEntry) {
	sessionsCacheMu.Lock()
	sessionsCache = entry
	sessionsCacheMu.Unlock()
}

func ActiveSessionCount() int {
	entry, err := cachedSessions()
	if err != nil {
		return 0
	}

	now := time.Now().Unix()
	count := 0
	for _, s := range entry.file.Sessions {
		if s.ExpiresAt > now && s.TokenHash != "" {
			count++
		}
	}

	return count
}

var sessionsMu sync.Mutex

func saveSessions(sf *SessionsFile) error {
	path := sessionsPath()
	tmpPath := path + ".tmp"

	data, _ := json.Marshal(sf)
	if err := os.WriteFile(tmpPath, append(data, '\n'), 0o600); err != nil {
		return err
	}

	_ = os.Chmod(tmpPath, 0o600)

	if err := os.Rename(tmpPath, path); err != nil {
		return err
	}

	cacheSessions(newSessionsCacheEntry(path, *sf))

	return nil
}

func CreateSession(username string, role Role, ttl time.Duration) (*Session, error) {
	user, ok := storedUser(username)
	if !ok {
		return nil, fmt.Errorf("user '%s' %w", username, ErrUserNotFound)
	}

	sessionsMu.Lock()
	defer sessionsMu.Unlock()

	sf, err := loadSessions()
	if err != nil {
		return nil, err
	}

	now := time.Now().Unix()
	var active []Session
	for _, s := range sf.Sessions {
		if s.ExpiresAt > now && s.TokenHash != "" {
			active = append(active, s)
		}
	}

	sf.Sessions = active

	id, err := generateSessionID()
	if err != nil {
		return nil, err
	}

	session := Session{
		ID:          id,
		TokenHash:   hashSessionID(id),
		Username:    username,
		Role:        role,
		Credentials: credentialsFingerprint(user.Password),
		ExpiresAt:   now + int64(ttl.Seconds()),
	}

	sf.Sessions = append(sf.Sessions, session)

	if err := saveSessions(sf); err != nil {
		return nil, err
	}

	return &session, nil
}

func GetSession(id string) (*Session, error) {
	if id == "" {
		return nil, nil
	}

	entry, err := cachedSessions()
	if err != nil {
		return nil, err
	}

	hash := hashSessionID(id)
	i, ok := entry.byHash[hash]
	if !ok {
		return nil, nil
	}

	s := entry.file.Sessions[i]
	if s.ExpiresAt <= time.Now().Unix() || subtle.ConstantTimeCompare([]byte(s.TokenHash), []byte(hash)) != 1 {
		return nil, nil
	}

	found := &s
	user, ok := storedUser(found.Username)
	if !ok || subtle.ConstantTimeCompare([]byte(found.Credentials), []byte(credentialsFingerprint(user.Password))) != 1 {
		return nil, nil
	}

	found.ID = id
	found.Role = user.Role

	return found, nil
}

func removeExpiredSessions() {
	sessionsMu.Lock()
	defer sessionsMu.Unlock()

	if sf, err := loadSessions(); err == nil {
		now := time.Now().Unix()
		active := make([]Session, 0, len(sf.Sessions))
		for _, s := range sf.Sessions {
			if s.ExpiresAt > now && s.TokenHash != "" {
				active = append(active, s)
			}
		}

		sf.Sessions = active
		if err := saveSessions(sf); err != nil {
			securityLog.Error("cannot save the sessions file", "err", err)
		}
	}
}

func ProtectSessionsFile() {
	cfg := globals.GetConfig()
	_ = os.Chmod(fmt.Sprintf("%s/%s", cfg.Store.Folder, SessionsFilename), 0o600)
}

func DeleteAllSessions() error {
	sessionsMu.Lock()
	defer sessionsMu.Unlock()

	cacheSessions(nil)

	if err := os.Remove(sessionsPath()); err != nil && !os.IsNotExist(err) {
		return err
	}

	return nil
}

func DeleteSession(id string) error {
	sessionsMu.Lock()
	defer sessionsMu.Unlock()

	sf, err := loadSessions()
	if err != nil {
		return err
	}

	hash := hashSessionID(id)
	now := time.Now().Unix()
	var updated []Session
	for _, s := range sf.Sessions {
		if s.TokenHash != hash && s.ExpiresAt > now && s.TokenHash != "" {
			updated = append(updated, s)
		}
	}

	sf.Sessions = updated

	return saveSessions(sf)
}

func DeleteUserSessions(username string) error {
	return deleteUserSessionsExcept(username, "", "")
}

func deleteUserSessionsExcept(username, keptSessionID, credentials string) error {
	sessionsMu.Lock()
	defer sessionsMu.Unlock()

	sf, err := loadSessions()
	if err != nil {
		return err
	}

	keptHash := ""
	if keptSessionID != "" {
		keptHash = hashSessionID(keptSessionID)
	}

	changed := false
	updated := make([]Session, 0, len(sf.Sessions))
	for _, s := range sf.Sessions {
		if s.Username != username {
			updated = append(updated, s)
			continue
		}

		if keptHash != "" && s.TokenHash == keptHash {
			s.Credentials = credentials
			updated = append(updated, s)
			changed = true
			continue
		}

		changed = true
	}

	if !changed {
		return nil
	}

	sf.Sessions = updated

	return saveSessions(sf)
}
