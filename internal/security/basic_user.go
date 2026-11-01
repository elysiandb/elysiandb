package security

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"strings"
	"sync"
	"unicode"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
	"golang.org/x/crypto/bcrypt"
)

const (
	KeyFilename          = "users.key"
	SessionsFilename     = "sessions.json"
	UserEntity           = "_elysiandb_core_user"
	DefaultAdminUsername = "admin"
	DefaultAdminPassword = "admin"
	AdminPasswordEnvVar  = "ELYSIANDB_ADMIN_PASSWORD"
	MinPasswordLength    = 8
	MaxUsernameLength    = 128
)

var (
	ErrUserNotFound          = errors.New("not found")
	ErrInvalidRole           = errors.New("invalid role: expected 'admin' or 'user'")
	ErrDefaultAdminProtected = errors.New("the default admin user cannot be deleted")
	ErrLastAdmin             = errors.New("at least one admin user must remain")
	ErrUserAlreadyExists     = errors.New("user already exists")
	ErrInvalidUsername       = errors.New("invalid username: 1 to 128 characters, without spaces, control characters or '/'")
	ErrPasswordTooShort      = fmt.Errorf("password must be at least %d characters long", MinPasswordLength)
)

type Role string

const (
	RoleAdmin Role = "admin"
	RoleUser  Role = "user"
)

type BasicUser struct {
	Username string
	Password string
	Role     Role
}

func (u *BasicUser) ToHasedUser() (*BasicHashedUser, error) {
	key, err := CreateKeyFileOrGetKey()
	if err != nil {
		return nil, err
	}

	sum := sha256.Sum256([]byte(u.Password + key))
	hashedPassword, err := bcrypt.GenerateFromPassword(sum[:], bcrypt.DefaultCost)
	if err != nil {
		return nil, err
	}

	role := u.Role
	if role == "" {
		role = RoleUser
	}

	return &BasicHashedUser{
		Username: u.Username,
		Password: string(hashedPassword),
		Role:     role,
	}, nil
}

type BasicHashedUser struct {
	Username string `json:"username"`
	Password string `json:"password"`
	Role     Role   `json:"role"`
}

type UsersFile struct {
	Users []BasicHashedUser `json:"users"`
}

func (u *BasicHashedUser) ToDataMap() map[string]any {
	return map[string]any{
		"id":       u.Username,
		"username": u.Username,
		"password": u.Password,
		"role":     string(u.Role),
	}
}

func (u *BasicHashedUser) FromDataMap(data map[string]any) error {
	username, ok := data["username"].(string)
	if !ok {
		return errors.New("invalid username in user data")
	}

	password, ok := data["password"].(string)
	if !ok {
		return errors.New("invalid password in user data")
	}

	roleStr, ok := data["role"].(string)
	if !ok {
		return errors.New("invalid role in user data")
	}

	u.Username = username
	u.Password = password
	u.Role = Role(roleStr)

	return nil
}

func (u *BasicHashedUser) Save() error {
	err := engine.WriteEntity(UserEntity, u.ToDataMap())
	if len(err) == 0 {
		return nil
	}

	var result strings.Builder
	for i, e := range err {
		fmt.Fprintf(&result, "Error %d: %s\n", i, e.ToError())
	}

	return fmt.Errorf("%s", result.String())
}

func InitAdminUserIfNotExists() error {
	adminUser := engine.ReadEntityById(UserEntity, DefaultAdminUsername)
	if adminUser != nil {
		return nil
	}

	password := os.Getenv(AdminPasswordEnvVar)
	if password == "" {
		password = DefaultAdminPassword
	}

	user := &BasicUser{
		Username: DefaultAdminUsername,
		Password: password,
		Role:     RoleAdmin,
	}

	return CreateBasicUser(user)
}

func UserEntitySchema() map[string]any {
	return map[string]any{
		"id": map[string]any{
			"type":     "string",
			"required": true,
		},
		"username": map[string]any{
			"type":     "string",
			"required": true,
		},
		"password": map[string]any{
			"type":     "string",
			"required": true,
		},
		"role": map[string]any{
			"type":     "string",
			"required": true,
		},
	}
}

func InitBasicUsersStorage() error {
	exists := engine.EntityTypeExists(UserEntity)
	if exists {
		return nil
	}

	err := engine.CreateEntityType(UserEntity)
	if err != nil {
		return err
	}

	engine.UpdateEntitySchema(UserEntity, UserEntitySchema())

	return nil
}

func GetBasicUserByUsername(username string) (map[string]any, error) {
	err := InitBasicUsersStorage()
	if err != nil {
		return nil, err
	}

	u := engine.ReadEntityById(UserEntity, username)
	if u == nil {
		return nil, fmt.Errorf("user '%s' not found", username)
	}

	delete(u, "password")

	return u, nil
}

func ListBasicUsers() ([]map[string]any, error) {
	err := InitBasicUsersStorage()
	if err != nil {
		return nil, err
	}

	users := engine.ListEntities(
		UserEntity,
		0,
		0,
		"username",
		true,
		nil,
		"",
		"",
	)

	result := make([]map[string]any, 0, len(users))
	for _, u := range users {
		delete(u, "password")
		result = append(result, u)
	}

	return result, nil
}

func ValidateUsername(username string) error {
	if username == "" || len(username) > MaxUsernameLength || username == "." || username == ".." {
		return ErrInvalidUsername
	}

	for _, r := range username {
		if r == '/' || unicode.IsSpace(r) || unicode.IsControl(r) {
			return ErrInvalidUsername
		}
	}

	return nil
}

func ValidatePassword(password string) error {
	if len([]rune(password)) < MinPasswordLength {
		return ErrPasswordTooShort
	}

	return nil
}

func BasicUserExists(username string) bool {
	return engine.ReadEntityById(UserEntity, username) != nil
}

func CreateBasicUser(user *BasicUser) error {
	err := InitBasicUsersStorage()
	if err != nil {
		return err
	}

	hashedUser, err := user.ToHasedUser()
	if err != nil {
		return err
	}

	err = hashedUser.Save()
	if err != nil {
		return err
	}

	return nil
}

func ChangeUserPassword(username, newPassword string) error {
	return changeUserPassword(username, newPassword, "")
}

func ChangeUserPasswordKeepingSession(username, newPassword, sessionID string) error {
	return changeUserPassword(username, newPassword, sessionID)
}

func changeUserPassword(username, newPassword, keptSessionID string) error {
	u := engine.ReadEntityById(UserEntity, username)
	if u == nil {
		return fmt.Errorf("user '%s' not found", username)
	}

	user := &BasicHashedUser{}
	err := user.FromDataMap(u)
	if err != nil {
		return err
	}

	key, err := CreateKeyFileOrGetKey()
	if err != nil {
		return err
	}

	sum := sha256.Sum256([]byte(newPassword + key))
	hashedPassword, err := bcrypt.GenerateFromPassword(sum[:], bcrypt.DefaultCost)
	if err != nil {
		return err
	}

	user.Password = string(hashedPassword)

	err = user.Save()
	if err != nil {
		return err
	}

	return deleteUserSessionsExcept(username, keptSessionID, credentialsFingerprint(user.Password))
}

func ChangeUserRole(username, newRole string) error {
	u := engine.ReadEntityById(UserEntity, username)
	if u == nil {
		return fmt.Errorf("user '%s' %w", username, ErrUserNotFound)
	}

	if Role(newRole) != RoleAdmin && Role(newRole) != RoleUser {
		return ErrInvalidRole
	}

	user := &BasicHashedUser{}
	err := user.FromDataMap(u)
	if err != nil {
		return err
	}

	if Role(newRole) != RoleAdmin && isLastAdmin(username) {
		return ErrLastAdmin
	}

	roleChanged := user.Role != Role(newRole)
	user.Role = Role(newRole)

	err = user.Save()
	if err != nil {
		return err
	}

	if roleChanged {
		return DeleteUserSessions(username)
	}

	return nil
}

func DeleteBasicUser(username string) error {
	if username == DefaultAdminUsername {
		return ErrDefaultAdminProtected
	}

	if isLastAdmin(username) {
		return ErrLastAdmin
	}

	engine.DeleteEntityById(UserEntity, username)

	return DeleteUserSessions(username)
}

func isLastAdmin(username string) bool {
	u := engine.ReadEntityById(UserEntity, username)
	if u == nil || u["role"] != string(RoleAdmin) {
		return false
	}

	users, _ := ListBasicUsers()

	admins := 0
	for _, user := range users {
		if user["role"] == string(RoleAdmin) {
			admins++
		}
	}

	return admins <= 1
}

func GenerateKey() (string, error) {
	b := make([]byte, 512)
	_, err := rand.Read(b)
	if err != nil {
		return "", fmt.Errorf("failed to generate key: %w", err)
	}

	return hex.EncodeToString(b), nil
}

func CreateKeyFileOrGetKey() (string, error) {
	cfg := globals.GetConfig()
	path := fmt.Sprintf("%s/%s", cfg.Store.Folder, KeyFilename)

	file, err := os.OpenFile(path, os.O_RDWR|os.O_CREATE, 0o600)
	if err != nil {
		return "", err
	}
	defer file.Close()

	_ = file.Chmod(0o600)

	fileInfo, err := file.Stat()
	if err != nil {
		return "", err
	}

	if fileInfo.Size() == 0 {
		key, err := GenerateKey()
		if err != nil {
			return "", err
		}

		if _, err := file.WriteAt([]byte(key), 0); err != nil {
			return "", err
		}

		return key, nil
	}

	if _, err := file.Seek(0, 0); err != nil {
		return "", err
	}

	var key string
	_, err = fmt.Fscanf(file, "%s", &key)
	if err != nil {
		return "", err
	}

	return key, nil
}

func WriteKey(key string) error {
	cfg := globals.GetConfig()
	path := fmt.Sprintf("%s/%s", cfg.Store.Folder, KeyFilename)
	tmpPath := path + ".tmp"

	if err := os.MkdirAll(cfg.Store.Folder, 0o755); err != nil {
		return err
	}

	if err := os.WriteFile(tmpPath, []byte(key), 0o600); err != nil {
		return err
	}

	return os.Rename(tmpPath, path)
}

var (
	dummyPasswordHashOnce sync.Once
	dummyPasswordHash     []byte
)

func spendPasswordCheck(password string) {
	dummyPasswordHashOnce.Do(func() {
		dummyPasswordHash, _ = bcrypt.GenerateFromPassword([]byte("elysiandb-dummy-password"), bcrypt.DefaultCost)
	})

	sum := sha256.Sum256([]byte(password))
	_ = bcrypt.CompareHashAndPassword(dummyPasswordHash, sum[:])
}

func AuthenticateUser(username, password string) (*BasicHashedUser, bool) {
	u := engine.ReadEntityById(UserEntity, username)
	if u == nil {
		spendPasswordCheck(password)
		return nil, false
	}

	user := &BasicHashedUser{}
	err := user.FromDataMap(u)
	if err != nil {
		spendPasswordCheck(password)
		return nil, false
	}

	key, err := CreateKeyFileOrGetKey()
	if err != nil {
		return nil, false
	}

	sum := sha256.Sum256([]byte(password + key))
	pass := sum[:]

	if bcrypt.CompareHashAndPassword([]byte(user.Password), pass) == nil {
		return user, true
	}

	return nil, false
}

var CheckBasicAuthentication = func(ctx *fasthttp.RequestCtx) bool {
	header := string(ctx.Request.Header.Peek("Authorization"))
	if header == "" || !strings.HasPrefix(header, "Basic ") {
		return false
	}

	payload, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(header, "Basic "))
	if err != nil {
		return false
	}

	parts := strings.SplitN(string(payload), ":", 2)
	if len(parts) != 2 {
		return false
	}

	username := parts[0]
	password := parts[1]

	_, _, ok := AuthenticateUserFrom(username, password, LoginSourceFromRequest(ctx))

	return ok
}
