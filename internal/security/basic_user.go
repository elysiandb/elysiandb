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

	api_storage "github.com/taymour/elysiandb/internal/api"
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

func (u *BasicHashedUser) ToDataMap() map[string]interface{} {
	return map[string]interface{}{
		"id":       u.Username,
		"username": u.Username,
		"password": u.Password,
		"role":     string(u.Role),
	}
}

func (u *BasicHashedUser) fromDataMap(data map[string]interface{}) error {
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
	err := api_storage.WriteEntity(UserEntity, u.ToDataMap())
	if len(err) == 0 {
		return nil
	}

	var result string
	for i, e := range err {
		result += fmt.Sprintf("Error %d: %s\n", i, e.ToError())
	}

	return fmt.Errorf("%s", result)
}

func InitAdminUserIfNotExists() error {
	adminUser := api_storage.ReadEntityById(UserEntity, DefaultAdminUsername)
	if adminUser != nil {
		return nil
	}

	user := &BasicUser{
		Username: DefaultAdminUsername,
		Password: DefaultAdminPassword,
		Role:     RoleAdmin,
	}

	return CreateBasicUser(user)
}

func UserEntitySchema() map[string]interface{} {
	return map[string]interface{}{
		"id": map[string]interface{}{
			"type":     "string",
			"required": true,
		},
		"username": map[string]interface{}{
			"type":     "string",
			"required": true,
		},
		"password": map[string]interface{}{
			"type":     "string",
			"required": true,
		},
		"role": map[string]interface{}{
			"type":     "string",
			"required": true,
		},
	}
}

func InitBasicUsersStorage() error {
	exists := api_storage.EntityTypeExists(UserEntity)
	if exists {
		return nil
	}

	err := api_storage.CreateEntityType(UserEntity)
	if err != nil {
		return err
	}

	api_storage.UpdateEntitySchema(UserEntity, UserEntitySchema())

	return nil
}

func GetBasicUserByUsername(username string) (map[string]interface{}, error) {
	err := InitBasicUsersStorage()
	if err != nil {
		return nil, err
	}

	u := api_storage.ReadEntityById(UserEntity, username)
	if u == nil {
		return nil, fmt.Errorf("user '%s' not found", username)
	}

	delete(u, "password")

	return u, nil
}

func ListBasicUsers() ([]map[string]interface{}, error) {
	err := InitBasicUsersStorage()
	if err != nil {
		return nil, err
	}

	users := api_storage.ListEntities(
		UserEntity,
		0,
		0,
		"username",
		true,
		nil,
		"",
		"",
	)

	result := make([]map[string]interface{}, 0, len(users))
	for _, u := range users {
		delete(u, "password")
		result = append(result, u)
	}

	return result, nil
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
	u := api_storage.ReadEntityById(UserEntity, username)
	if u == nil {
		return fmt.Errorf("user '%s' not found", username)
	}

	user := &BasicHashedUser{}
	err := user.fromDataMap(u)
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

	return nil
}

func DeleteBasicUser(username string) {
	if username == DefaultAdminUsername {
		return
	}

	api_storage.DeleteEntityById(UserEntity, username)
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

	file, err := os.OpenFile(path, os.O_RDWR|os.O_CREATE, 0644)
	if err != nil {
		return "", err
	}
	defer file.Close()

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

func AuthenticateUser(username, password string) (*BasicHashedUser, bool) {
	u := api_storage.ReadEntityById(UserEntity, username)
	if u == nil {
		return nil, false
	}

	user := &BasicHashedUser{}
	err := user.fromDataMap(u)
	if err != nil {
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

	_, ok := AuthenticateUser(username, password)
	return ok
}
